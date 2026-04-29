use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const AUGMENT_TIERS: [u8; 4] = [3, 7, 11, 15];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LivePlayerSnapshot {
    pub champion_name: String,
    pub level: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PanelState {
    Expanded,
    Collapsed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AugmentChoice {
    pub slot: u8,
    pub augment_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PauseReason {
    LiveClientUnavailable,
    InvalidLiveClientData,
    InvalidPanelData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateMachineInput {
    pub player: Option<LivePlayerSnapshot>,
    pub panel_state: PanelState,
    pub choices: Vec<AugmentChoice>,
    pub selected_slot: Option<u8>,
    pub pause_reason: Option<PauseReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AssistantStatus {
    WaitingForGame,
    WaitingForTier,
    PendingSelection,
    Paused,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StateEventKind {
    PlayerChanged,
    LevelChanged,
    TierPending,
    PanelExpanded,
    PanelCollapsed,
    SlotRefreshed,
    TierCompleted,
    Paused,
    Resumed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateTransitionEvent {
    pub kind: StateEventKind,
    pub from_status: AssistantStatus,
    pub to_status: AssistantStatus,
    pub tier: Option<u8>,
    pub slot: Option<u8>,
    pub previous_value: Option<String>,
    pub next_value: Option<String>,
    pub reason: Option<PauseReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantState {
    pub status: AssistantStatus,
    pub player: Option<LivePlayerSnapshot>,
    pub pending_tier: Option<u8>,
    pub pending_tiers: Vec<u8>,
    pub completed_tiers: BTreeSet<u8>,
    pub panel_state: PanelState,
    pub visible_choices: BTreeMap<u8, String>,
    pub pause_reason: Option<PauseReason>,
}

impl Default for AssistantState {
    fn default() -> Self {
        Self {
            status: AssistantStatus::WaitingForGame,
            player: None,
            pending_tier: None,
            pending_tiers: Vec::new(),
            completed_tiers: BTreeSet::new(),
            panel_state: PanelState::Collapsed,
            visible_choices: BTreeMap::new(),
            pause_reason: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct AssistantStateMachine {
    state: AssistantState,
}

impl AssistantStateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> &AssistantState {
        &self.state
    }

    pub fn apply(&mut self, input: StateMachineInput) -> Vec<StateTransitionEvent> {
        let mut events = Vec::new();
        let starting_status = self.state.status;

        if let Some(reason) = input.pause_reason {
            self.state.pause_reason = Some(reason.clone());
            self.state.status = AssistantStatus::Paused;
            events.push(self.event(
                StateEventKind::Paused,
                starting_status,
                AssistantStatus::Paused,
                None,
                None,
                None,
                None,
                Some(reason),
            ));
            return events;
        }

        if self.state.status == AssistantStatus::Paused {
            self.state.pause_reason = None;
            events.push(self.event(
                StateEventKind::Resumed,
                AssistantStatus::Paused,
                AssistantStatus::WaitingForTier,
                self.state.pending_tier,
                None,
                None,
                None,
                None,
            ));
        }

        self.apply_player(input.player, &mut events);
        self.apply_panel_state(input.panel_state, &mut events);

        let eligible_tier = self
            .state
            .player
            .as_ref()
            .map(|player| pending_tiers(player.level, &self.state.completed_tiers))
            .unwrap_or_default();
        self.state.pending_tiers = eligible_tier.clone();

        if let Some(tier) = eligible_tier.first().copied() {
            self.ensure_pending_tier(tier, &mut events);
        } else {
            self.state.pending_tier = None;
            self.state.pending_tiers.clear();
            self.state.visible_choices.clear();
        }

        if self.state.panel_state == PanelState::Expanded {
            self.refresh_choices(input.choices, &mut events);
        } else {
            self.state.visible_choices.clear();
        }

        if let (Some(tier), Some(slot)) = (self.state.pending_tier, input.selected_slot) {
            if self.state.panel_state == PanelState::Expanded
                && self.state.visible_choices.contains_key(&slot)
            {
                let from_status = self.state.status;
                self.state.completed_tiers.insert(tier);
                self.state.pending_tiers = self
                    .state
                    .player
                    .as_ref()
                    .map(|player| pending_tiers(player.level, &self.state.completed_tiers))
                    .unwrap_or_default();
                self.state.pending_tier = self.state.pending_tiers.first().copied();
                self.state.visible_choices.clear();
                self.state.status = status_after_completion(
                    self.state.player.as_ref().map(|player| player.level),
                    &self.state.completed_tiers,
                );
                events.push(self.event(
                    StateEventKind::TierCompleted,
                    from_status,
                    self.state.status,
                    Some(tier),
                    Some(slot),
                    None,
                    None,
                    None,
                ));
            }
        }

        if self.state.status != AssistantStatus::PendingSelection
            && self.state.pending_tier.is_some()
        {
            self.state.status = AssistantStatus::PendingSelection;
        } else if self.state.pending_tier.is_none() && self.state.player.is_some() {
            self.state.status = AssistantStatus::WaitingForTier;
        } else if self.state.player.is_none() {
            self.state.status = AssistantStatus::WaitingForGame;
        }

        events
    }

    fn apply_player(
        &mut self,
        player: Option<LivePlayerSnapshot>,
        events: &mut Vec<StateTransitionEvent>,
    ) {
        let previous_player = self.state.player.clone();
        self.state.player = player;

        match (&previous_player, &self.state.player) {
            (Some(previous), Some(next)) if previous.champion_name != next.champion_name => {
                self.state.completed_tiers.clear();
                self.state.pending_tier = None;
                self.state.pending_tiers.clear();
                self.state.visible_choices.clear();
                events.push(self.event(
                    StateEventKind::PlayerChanged,
                    self.state.status,
                    AssistantStatus::WaitingForTier,
                    None,
                    None,
                    Some(previous.champion_name.clone()),
                    Some(next.champion_name.clone()),
                    None,
                ));
            }
            (None, Some(next)) => {
                events.push(self.event(
                    StateEventKind::PlayerChanged,
                    self.state.status,
                    AssistantStatus::WaitingForTier,
                    None,
                    None,
                    None,
                    Some(next.champion_name.clone()),
                    None,
                ));
            }
            _ => {}
        }

        if let (Some(previous), Some(next)) = (&previous_player, &self.state.player) {
            if previous.level != next.level {
                events.push(self.event(
                    StateEventKind::LevelChanged,
                    self.state.status,
                    self.state.status,
                    None,
                    None,
                    Some(previous.level.to_string()),
                    Some(next.level.to_string()),
                    None,
                ));
            }
        }
    }

    fn apply_panel_state(
        &mut self,
        panel_state: PanelState,
        events: &mut Vec<StateTransitionEvent>,
    ) {
        if self.state.panel_state == panel_state {
            return;
        }

        let kind = match panel_state {
            PanelState::Expanded => StateEventKind::PanelExpanded,
            PanelState::Collapsed => StateEventKind::PanelCollapsed,
        };

        let previous = self.state.panel_state;
        self.state.panel_state = panel_state;
        events.push(self.event(
            kind,
            self.state.status,
            self.state.status,
            self.state.pending_tier,
            None,
            Some(format!("{previous:?}")),
            Some(format!("{panel_state:?}")),
            None,
        ));
    }

    fn ensure_pending_tier(&mut self, tier: u8, events: &mut Vec<StateTransitionEvent>) {
        if self.state.pending_tier == Some(tier) {
            self.state.status = AssistantStatus::PendingSelection;
            return;
        }

        let from_status = self.state.status;
        self.state.pending_tier = Some(tier);
        self.state.status = AssistantStatus::PendingSelection;
        events.push(self.event(
            StateEventKind::TierPending,
            from_status,
            AssistantStatus::PendingSelection,
            Some(tier),
            None,
            None,
            None,
            None,
        ));
    }

    fn refresh_choices(
        &mut self,
        choices: Vec<AugmentChoice>,
        events: &mut Vec<StateTransitionEvent>,
    ) {
        let next_choices = choices
            .into_iter()
            .map(|choice| (choice.slot, choice.augment_id))
            .collect::<BTreeMap<_, _>>();

        for (slot, next_value) in &next_choices {
            if self.state.visible_choices.get(slot) != Some(next_value) {
                events.push(self.event(
                    StateEventKind::SlotRefreshed,
                    self.state.status,
                    self.state.status,
                    self.state.pending_tier,
                    Some(*slot),
                    self.state.visible_choices.get(slot).cloned(),
                    Some(next_value.clone()),
                    None,
                ));
            }
        }

        self.state.visible_choices = next_choices;
    }

    fn event(
        &self,
        kind: StateEventKind,
        from_status: AssistantStatus,
        to_status: AssistantStatus,
        tier: Option<u8>,
        slot: Option<u8>,
        previous_value: Option<String>,
        next_value: Option<String>,
        reason: Option<PauseReason>,
    ) -> StateTransitionEvent {
        StateTransitionEvent {
            kind,
            from_status,
            to_status,
            tier,
            slot,
            previous_value,
            next_value,
            reason,
        }
    }
}

fn next_pending_tier(level: u8, completed_tiers: &BTreeSet<u8>) -> Option<u8> {
    AUGMENT_TIERS
        .iter()
        .copied()
        .find(|tier| level >= *tier && !completed_tiers.contains(tier))
}

fn pending_tiers(level: u8, completed_tiers: &BTreeSet<u8>) -> Vec<u8> {
    AUGMENT_TIERS
        .iter()
        .copied()
        .filter(|tier| level >= *tier && !completed_tiers.contains(tier))
        .collect()
}

fn status_after_completion(level: Option<u8>, completed_tiers: &BTreeSet<u8>) -> AssistantStatus {
    match level.and_then(|value| next_pending_tier(value, completed_tiers)) {
        Some(_) => AssistantStatus::PendingSelection,
        None => AssistantStatus::WaitingForTier,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(level: u8) -> LivePlayerSnapshot {
        LivePlayerSnapshot {
            champion_name: "Ahri".to_string(),
            level,
        }
    }

    fn choice(slot: u8, id: &str) -> AugmentChoice {
        AugmentChoice {
            slot,
            augment_id: id.to_string(),
        }
    }

    fn input(level: u8, panel_state: PanelState, choices: Vec<AugmentChoice>) -> StateMachineInput {
        StateMachineInput {
            player: Some(player(level)),
            panel_state,
            choices,
            selected_slot: None,
            pause_reason: None,
        }
    }

    #[test]
    fn tracks_multiple_pending_tiers() {
        let mut machine = AssistantStateMachine::new();

        machine.apply(input(
            3,
            PanelState::Expanded,
            vec![choice(0, "tier3-a"), choice(1, "tier3-b")],
        ));
        assert_eq!(machine.state().pending_tier, Some(3));

        machine.apply(StateMachineInput {
            selected_slot: Some(0),
            ..input(
                3,
                PanelState::Expanded,
                vec![choice(0, "tier3-a"), choice(1, "tier3-b")],
            )
        });
        assert!(machine.state().completed_tiers.contains(&3));

        machine.apply(input(
            11,
            PanelState::Expanded,
            vec![choice(0, "tier7-a"), choice(1, "tier7-b")],
        ));
        assert_eq!(machine.state().pending_tier, Some(7));

        machine.apply(StateMachineInput {
            selected_slot: Some(1),
            ..input(
                11,
                PanelState::Expanded,
                vec![choice(0, "tier7-a"), choice(1, "tier7-b")],
            )
        });
        assert!(machine.state().completed_tiers.contains(&7));

        machine.apply(input(
            11,
            PanelState::Expanded,
            vec![choice(0, "tier11-a"), choice(1, "tier11-b")],
        ));
        assert_eq!(machine.state().pending_tier, Some(11));
    }

    #[test]
    fn collapsed_panel_does_not_complete_pending_tier() {
        let mut machine = AssistantStateMachine::new();

        machine.apply(input(
            7,
            PanelState::Collapsed,
            vec![choice(0, "tier3-a"), choice(1, "tier3-b")],
        ));

        assert_eq!(machine.state().pending_tier, Some(3));
        assert!(machine.state().completed_tiers.is_empty());
        assert_eq!(machine.state().visible_choices.len(), 0);

        machine.apply(StateMachineInput {
            selected_slot: Some(0),
            ..input(
                7,
                PanelState::Collapsed,
                vec![choice(0, "tier3-a"), choice(1, "tier3-b")],
            )
        });

        assert_eq!(machine.state().pending_tier, Some(3));
        assert!(machine.state().completed_tiers.is_empty());
    }

    #[test]
    fn reroll_refreshes_only_changed_slot() {
        let mut machine = AssistantStateMachine::new();

        machine.apply(input(
            3,
            PanelState::Expanded,
            vec![
                choice(0, "stable-a"),
                choice(1, "old-b"),
                choice(2, "stable-c"),
            ],
        ));

        let events = machine.apply(input(
            3,
            PanelState::Expanded,
            vec![
                choice(0, "stable-a"),
                choice(1, "new-b"),
                choice(2, "stable-c"),
            ],
        ));

        let refreshed_slots = events
            .iter()
            .filter(|event| event.kind == StateEventKind::SlotRefreshed)
            .map(|event| event.slot)
            .collect::<Vec<_>>();

        assert_eq!(refreshed_slots, vec![Some(1)]);
        assert_eq!(
            machine.state().visible_choices.get(&1),
            Some(&"new-b".to_string())
        );
    }
}
