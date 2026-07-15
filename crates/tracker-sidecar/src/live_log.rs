use domain::{CardInstanceId, CardKey, CardKnowledge, Zone};
use state_reader::{TextOverlayCard, TextOverlayPayload, TextOverlaySlotState};
use std::collections::{HashMap, HashSet};

const ENABLE_LIVE_COUNTER_INFERENCE: bool = false;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiveLogEvent {
    PlayerDrew {
        key: CardKey,
    },
    PlayerHandObserved {
        key: CardKey,
    },
    PlayerStaged {
        key: CardKey,
        entity_id: Option<String>,
        turn: Option<u8>,
    },
    PlayerStageAccepted {
        entity_id: Option<String>,
        undo: bool,
    },
    CardResolved {
        key: CardKey,
    },
    CardDestroyedTrigger {
        key: CardKey,
    },
    CardDiscardedTrigger {
        key: CardKey,
    },
    OpponentDiscardClue,
    OpponentCardDiscarded {
        key: CardKey,
    },
    HiddenZoneChangeClue {
        reason: String,
    },
    PlayerCardRemoved {
        key: CardKey,
    },
    LocationRevealed {
        key: String,
    },
    ResolutionStarted,
    TurnStarted,
    MatchStarted {
        game_id: Option<String>,
    },
    MatchEnded,
    MatchLeft,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LiveLogState {
    player_deck_keys: HashSet<CardKey>,
    player_consumed_keys: HashSet<CardKey>,
    player_away_keys: Vec<CardKey>,
    player_hand_keys: Vec<CardKey>,
    player_supplemental: Vec<LiveSupplementalCard>,
    player_destroyed: Vec<CardKey>,
    player_discarded: Vec<CardKey>,
    player_returned_discard_signals: Vec<CardKey>,
    player_discard_seen: bool,
    player_removed: Vec<CardKey>,
    opponent_supplemental: Vec<LiveSupplementalCard>,
    opponent_known: Vec<CardKey>,
    opponent_consumed_keys: HashSet<CardKey>,
    opponent_discarded: Vec<CardKey>,
    staged_by_entity: HashMap<String, CardKey>,
    recently_player_resolved: HashSet<CardKey>,
    pending_hand_swap: bool,
    pending_hand_swap_hand_events: u8,
    player_counts: Option<LivePanelCounts>,
    opponent_counts: Option<LivePanelCounts>,
    turn_starts_seen: u8,
}

impl LiveLogState {
    pub fn set_player_deck<I>(&mut self, keys: I)
    where
        I: IntoIterator<Item = CardKey>,
    {
        self.player_deck_keys = keys.into_iter().collect();
    }

    #[allow(dead_code)]
    pub fn has_player_deck_card(&self, key: &CardKey) -> bool {
        self.player_deck_keys.contains(key)
    }

    #[allow(dead_code)]
    pub fn has_player_supplemental_card(&self, key: &CardKey) -> bool {
        self.player_supplemental.iter().any(|card| &card.key == key)
    }

    pub fn seed_counters_from_payload(&mut self, payload: &TextOverlayPayload) {
        if !ENABLE_LIVE_COUNTER_INFERENCE {
            return;
        }
        if self.player_counts.is_none()
            && (payload.player.counters.deck > 0 || payload.player.counters.hand > 0)
        {
            self.player_counts = Some(LivePanelCounts::from_payload(
                payload.player.counters.deck,
                payload.player.counters.hand,
            ));
        }
        if self.opponent_counts.is_none()
            && (payload.opponent.counters.deck > 0 || payload.opponent.counters.hand > 0)
        {
            self.opponent_counts = Some(LivePanelCounts::from_payload(
                payload.opponent.counters.deck,
                payload.opponent.counters.hand,
            ));
        }
    }

    pub fn apply(&mut self, event: LiveLogEvent) -> bool {
        match event {
            LiveLogEvent::PlayerDrew { key } => {
                if ENABLE_LIVE_COUNTER_INFERENCE {
                    self.player_counts_mut().draw_from_deck();
                }
                self.record_player_card_seen(key, PlayerCardSeenSource::Hand);
                true
            }
            LiveLogEvent::PlayerHandObserved { key } => {
                self.record_player_card_seen(key, PlayerCardSeenSource::Hand);
                true
            }
            LiveLogEvent::PlayerStaged {
                key,
                entity_id,
                turn: _,
            } => {
                if let Some(entity_id) = entity_id
                    && self
                        .staged_by_entity
                        .insert(entity_id, key.clone())
                        .is_none()
                    && ENABLE_LIVE_COUNTER_INFERENCE
                {
                    self.player_counts_mut().play_from_hand();
                }
                self.record_player_card_seen(key, PlayerCardSeenSource::Staged);
                true
            }
            LiveLogEvent::PlayerStageAccepted { entity_id, undo } => {
                if undo {
                    if let Some(entity_id) = entity_id
                        && self.staged_by_entity.remove(&entity_id).is_some()
                        && ENABLE_LIVE_COUNTER_INFERENCE
                    {
                        self.player_counts_mut().return_to_hand();
                    }
                    return true;
                }
                if let Some(entity_id) = entity_id
                    && let Some(key) = self.staged_by_entity.get(&entity_id).cloned()
                {
                    self.record_player_card_seen(key, PlayerCardSeenSource::Staged);
                }
                true
            }
            LiveLogEvent::CardResolved { key } => {
                if self.recently_player_resolved.remove(&key) {
                    return true;
                }
                if ENABLE_LIVE_COUNTER_INFERENCE {
                    self.opponent_counts_mut().play_from_hand();
                }
                if let Some(card) = self
                    .opponent_supplemental
                    .iter_mut()
                    .find(|card| card.key == key)
                {
                    card.zone = Zone::Board;
                    card.consumed = true;
                    self.opponent_consumed_keys.insert(key);
                    return true;
                }
                self.opponent_consumed_keys.insert(key.clone());
                push_unique(&mut self.opponent_known, key);
                true
            }
            LiveLogEvent::CardDestroyedTrigger { key } => {
                if self.player_deck_keys.contains(&key) {
                    push_unique(&mut self.player_destroyed, key.clone());
                    self.player_consumed_keys.insert(key);
                }
                true
            }
            LiveLogEvent::CardDiscardedTrigger { key } => {
                if ENABLE_LIVE_COUNTER_INFERENCE {
                    self.player_counts_mut().play_from_hand();
                }
                self.record_player_card_discarded(key);
                true
            }
            LiveLogEvent::OpponentDiscardClue => false,
            LiveLogEvent::OpponentCardDiscarded { key } => {
                if ENABLE_LIVE_COUNTER_INFERENCE {
                    self.opponent_counts_mut().play_from_hand();
                }
                push_unique(&mut self.opponent_discarded, key.clone());
                self.opponent_consumed_keys.insert(key.clone());
                push_unique(&mut self.opponent_known, key);
                true
            }
            LiveLogEvent::HiddenZoneChangeClue { .. } => false,
            LiveLogEvent::PlayerCardRemoved { key } => {
                if self.player_deck_keys.contains(&key) {
                    if ENABLE_LIVE_COUNTER_INFERENCE {
                        self.player_counts_mut().remove_from_deck();
                    }
                    self.player_consumed_keys.insert(key.clone());
                }
                push_unique(&mut self.player_removed, key);
                true
            }
            LiveLogEvent::LocationRevealed { key } => {
                if key == "ThePeak" {
                    self.pending_hand_swap = true;
                    self.pending_hand_swap_hand_events = 0;
                }
                true
            }
            LiveLogEvent::ResolutionStarted => false,
            LiveLogEvent::TurnStarted => {
                self.turn_starts_seen = self.turn_starts_seen.saturating_add(1);
                if ENABLE_LIVE_COUNTER_INFERENCE && self.turn_starts_seen > 1 {
                    self.opponent_counts_mut().draw_from_deck();
                }
                true
            }
            LiveLogEvent::MatchStarted { .. } => {
                self.clear_match_state();
                true
            }
            LiveLogEvent::MatchEnded => false,
            LiveLogEvent::MatchLeft => {
                self.clear_match_state();
                true
            }
        }
    }

    pub fn apply_to_payload(&self, payload: &mut TextOverlayPayload) {
        self.apply_to_payload_with_options(
            payload,
            ApplyOptions {
                preserve_snapshot_buckets: true,
            },
        );
    }

    pub fn apply_to_snapshot_payload(&self, payload: &mut TextOverlayPayload) {
        self.apply_to_payload_with_options(
            payload,
            ApplyOptions {
                preserve_snapshot_buckets: true,
            },
        );
    }

    fn apply_to_payload_with_options(
        &self,
        payload: &mut TextOverlayPayload,
        options: ApplyOptions,
    ) {
        let mut player_known_original = 0usize;
        let mut player_consumed_original = 0usize;
        for slot in &mut payload.player.deck_slots {
            if let Some(card) = &mut slot.card {
                player_known_original += 1;
                if card
                    .card_definition_key
                    .as_ref()
                    .is_some_and(|key| self.player_away_keys.contains(key))
                {
                    card.zone = Zone::UnknownTransition;
                    card.raw_zone = Some("Player.log:away".to_string());
                }
                if card
                    .card_definition_key
                    .as_ref()
                    .is_some_and(|key| self.player_consumed_keys.contains(key))
                {
                    card.consumed_from_deck = true;
                    player_consumed_original += 1;
                }
            }
        }
        if ENABLE_LIVE_COUNTER_INFERENCE && let Some(counts) = self.player_counts {
            payload.player.counters.deck = counts.deck();
            payload.player.counters.hand = counts.hand();
        } else if player_known_original > 0 {
            payload.player.counters.deck =
                player_known_original.saturating_sub(player_consumed_original);
            payload.player.counters.hand = self.player_hand_keys.len();
        }

        for supplemental in &self.player_supplemental {
            if !payload
                .player
                .supplemental
                .iter()
                .any(|card| card.card_definition_key.as_ref() == Some(&supplemental.key))
            {
                payload.player.supplemental.push(live_text_card(
                    "player-log:supplemental",
                    payload.player.supplemental.len(),
                    &supplemental.key,
                    supplemental.zone,
                    false,
                    supplemental.consumed,
                ));
            } else if let Some(card) = payload
                .player
                .supplemental
                .iter_mut()
                .find(|card| card.card_definition_key.as_ref() == Some(&supplemental.key))
            {
                card.zone = supplemental.zone;
                card.consumed_from_deck = supplemental.consumed;
            }
        }
        payload.player.counters.supplemental = payload.player.supplemental.len();

        for key in &self.player_destroyed {
            if !payload
                .player
                .destroyed
                .iter()
                .any(|card| card.card_definition_key.as_ref() == Some(key))
            {
                payload.player.destroyed.push(live_text_card(
                    "player-log:destroyed",
                    payload.player.destroyed.len(),
                    key,
                    Zone::Destroyed,
                    true,
                    true,
                ));
            }
        }
        payload.player.counters.destroyed = payload.player.destroyed.len();

        if self.player_discard_seen && !options.preserve_snapshot_buckets {
            payload.player.discarded.clear();
        }
        if self.player_discard_seen {
            for key in &self.player_discarded {
                if !payload
                    .player
                    .discarded
                    .iter()
                    .any(|card| card.card_definition_key.as_ref() == Some(key))
                {
                    payload.player.discarded.push(live_text_card(
                        "player-log:discarded",
                        payload.player.discarded.len(),
                        key,
                        Zone::Discarded,
                        true,
                        true,
                    ));
                }
            }
        }
        if !self.player_returned_discard_signals.is_empty() {
            payload.player.discarded.retain(|card| {
                card.card_definition_key.as_ref().is_none_or(|key| {
                    !self
                        .player_returned_discard_signals
                        .iter()
                        .any(|signal| discard_returned_as(key, signal))
                })
            });
        }
        payload.player.counters.discarded = payload.player.discarded.len();

        for key in &self.player_removed {
            if !payload
                .player
                .removed
                .iter()
                .any(|card| card.card_definition_key.as_ref() == Some(key))
            {
                payload.player.removed.push(live_text_card(
                    "player-log:removed",
                    payload.player.removed.len(),
                    key,
                    Zone::RemovedConfirmed,
                    true,
                    true,
                ));
            }
        }
        payload.player.counters.removed = payload.player.removed.len();

        for supplemental in &self.opponent_supplemental {
            if !payload
                .opponent
                .supplemental
                .iter()
                .any(|card| card.card_definition_key.as_ref() == Some(&supplemental.key))
            {
                payload.opponent.supplemental.push(live_text_card(
                    "player-log:opponent-supplemental",
                    payload.opponent.supplemental.len(),
                    &supplemental.key,
                    supplemental.zone,
                    false,
                    supplemental.consumed,
                ));
            } else if let Some(card) = payload
                .opponent
                .supplemental
                .iter_mut()
                .find(|card| card.card_definition_key.as_ref() == Some(&supplemental.key))
            {
                card.zone = supplemental.zone;
                card.consumed_from_deck = supplemental.consumed;
            }
        }
        self.normalize_supplemental_brightness(payload);
        payload.opponent.counters.supplemental = payload.opponent.supplemental.len();

        if !options.preserve_snapshot_buckets {
            payload.opponent.discarded.clear();
        }
        for key in &self.opponent_discarded {
            if !payload
                .opponent
                .discarded
                .iter()
                .any(|card| card.card_definition_key.as_ref() == Some(key))
            {
                payload.opponent.discarded.push(live_text_card(
                    "player-log:opponent-discarded",
                    payload.opponent.discarded.len(),
                    key,
                    Zone::Discarded,
                    true,
                    true,
                ));
            }
        }
        payload.opponent.counters.discarded = payload.opponent.discarded.len();

        for key in &self.opponent_known {
            if self
                .opponent_supplemental
                .iter()
                .any(|card| &card.key == key)
            {
                continue;
            }
            if let Some(card) = payload
                .opponent
                .deck_slots
                .iter_mut()
                .filter_map(|slot| slot.card.as_mut())
                .find(|card| card.card_definition_key.as_ref() == Some(key))
            {
                if self.opponent_consumed_keys.contains(key) {
                    card.consumed_from_deck = true;
                    card.zone = Zone::Board;
                }
                continue;
            }
            if payload.opponent.deck_slots.iter().any(|slot| {
                slot.card
                    .as_ref()
                    .and_then(|card| card.card_definition_key.as_ref())
                    == Some(key)
            }) {
                continue;
            }
            if let Some(slot) = payload
                .opponent
                .deck_slots
                .iter_mut()
                .find(|slot| slot.state == TextOverlaySlotState::Unknown)
            {
                let index = usize::from(slot.slot_index);
                slot.state = TextOverlaySlotState::Known;
                slot.card = Some(live_text_card(
                    "player-log:opponent",
                    index,
                    key,
                    Zone::Board,
                    true,
                    true,
                ));
            }
        }
        if ENABLE_LIVE_COUNTER_INFERENCE && let Some(counts) = self.opponent_counts {
            payload.opponent.counters.deck = counts.deck();
            payload.opponent.counters.hand = counts.hand();
        }
    }

    fn record_player_card_discarded(&mut self, key: CardKey) {
        self.player_discard_seen = true;
        if self_returning_discard(&key) {
            if self.player_deck_keys.contains(&key) {
                self.player_consumed_keys.insert(key.clone());
            }
            push_unique(&mut self.player_returned_discard_signals, key.clone());
            self.remove_returned_discard(&key);
            if ENABLE_LIVE_COUNTER_INFERENCE {
                self.player_counts_mut().return_to_hand();
            }
            push_unique(&mut self.player_hand_keys, key);
            return;
        }
        if self.player_deck_keys.contains(&key) {
            self.player_consumed_keys.insert(key.clone());
        }
        remove_first(&mut self.player_hand_keys, &key);
        push_unique(&mut self.player_discarded, key);
    }

    fn record_player_card_seen(&mut self, key: CardKey, source: PlayerCardSeenSource) {
        if source == PlayerCardSeenSource::Hand {
            self.remove_returned_discard(&key);
        }

        if self.pending_hand_swap && source == PlayerCardSeenSource::Hand {
            self.pending_hand_swap_hand_events =
                self.pending_hand_swap_hand_events.saturating_add(1);
            if !self.player_deck_keys.contains(&key) {
                if let Some(away_key) = self.player_hand_keys.first().cloned() {
                    push_unique(&mut self.player_away_keys, away_key.clone());
                    upsert_supplemental(
                        &mut self.opponent_supplemental,
                        LiveSupplementalCard {
                            key: away_key.clone(),
                            zone: Zone::Hand,
                            consumed: false,
                        },
                    );
                    self.player_consumed_keys.insert(away_key);
                    self.player_hand_keys.remove(0);
                }
                self.pending_hand_swap = false;
                self.pending_hand_swap_hand_events = 0;
            } else if self.pending_hand_swap_hand_events >= 4 {
                self.pending_hand_swap = false;
                self.pending_hand_swap_hand_events = 0;
            }
        }

        if self.player_deck_keys.contains(&key) {
            self.player_consumed_keys.insert(key.clone());
        } else {
            upsert_supplemental(
                &mut self.player_supplemental,
                LiveSupplementalCard {
                    key: key.clone(),
                    zone: source.zone(),
                    consumed: source.consumes_player_supplemental(),
                },
            );
        }
        match source {
            PlayerCardSeenSource::Hand => push_unique(&mut self.player_hand_keys, key),
            PlayerCardSeenSource::Staged => {
                remove_first(&mut self.player_hand_keys, &key);
                self.recently_player_resolved.insert(key);
            }
        }
    }

    pub fn reset_match(&mut self) {
        self.clear_match_state();
    }

    fn clear_match_state(&mut self) {
        self.player_consumed_keys.clear();
        self.player_away_keys.clear();
        self.player_hand_keys.clear();
        self.player_supplemental.clear();
        self.player_destroyed.clear();
        self.player_discarded.clear();
        self.player_returned_discard_signals.clear();
        self.player_discard_seen = false;
        self.player_removed.clear();
        self.opponent_supplemental.clear();
        self.opponent_known.clear();
        self.opponent_consumed_keys.clear();
        self.opponent_discarded.clear();
        self.staged_by_entity.clear();
        self.recently_player_resolved.clear();
        self.pending_hand_swap = false;
        self.pending_hand_swap_hand_events = 0;
        self.player_counts = None;
        self.opponent_counts = None;
        self.turn_starts_seen = 0;
    }

    fn remove_returned_discard(&mut self, observed_key: &CardKey) {
        if return_hand_signal(&observed_key.0) {
            push_unique(
                &mut self.player_returned_discard_signals,
                observed_key.clone(),
            );
        }
        self.player_discarded
            .retain(|discarded| !discard_returned_as(discarded, observed_key));
    }

    fn normalize_supplemental_brightness(&self, payload: &mut TextOverlayPayload) {
        for card in &mut payload.player.supplemental {
            card.consumed_from_deck = card.zone != Zone::Deck;
        }
        for card in &mut payload.opponent.supplemental {
            let live_consumed = card
                .card_definition_key
                .as_ref()
                .is_some_and(|key| self.opponent_consumed_keys.contains(key));
            card.consumed_from_deck =
                live_consumed || !matches!(card.zone, Zone::Deck | Zone::Hand);
        }
    }

    fn player_counts_mut(&mut self) -> &mut LivePanelCounts {
        self.player_counts
            .get_or_insert_with(LivePanelCounts::standard_opening)
    }

    fn opponent_counts_mut(&mut self) -> &mut LivePanelCounts {
        self.opponent_counts
            .get_or_insert_with(LivePanelCounts::standard_opening)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LivePanelCounts {
    deck: i16,
    hand: i16,
}

impl LivePanelCounts {
    fn standard_opening() -> Self {
        Self { deck: 8, hand: 4 }
    }

    fn from_payload(deck: usize, hand: usize) -> Self {
        Self {
            deck: i16::try_from(deck).unwrap_or(i16::MAX),
            hand: i16::try_from(hand).unwrap_or(i16::MAX),
        }
    }

    fn deck(self) -> usize {
        usize::try_from(self.deck.max(0)).unwrap_or(0)
    }

    fn hand(self) -> usize {
        usize::try_from(self.hand.max(0)).unwrap_or(0)
    }

    fn draw_from_deck(&mut self) {
        self.deck = self.deck.saturating_sub(1).max(0);
        self.hand = self.hand.saturating_add(1);
    }

    fn play_from_hand(&mut self) {
        self.hand = self.hand.saturating_sub(1).max(0);
    }

    fn return_to_hand(&mut self) {
        self.hand = self.hand.saturating_add(1);
    }

    fn remove_from_deck(&mut self) {
        self.deck = self.deck.saturating_sub(1).max(0);
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ApplyOptions {
    preserve_snapshot_buckets: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlayerCardSeenSource {
    Hand,
    Staged,
}

impl PlayerCardSeenSource {
    fn zone(self) -> Zone {
        match self {
            Self::Hand => Zone::Hand,
            Self::Staged => Zone::Board,
        }
    }

    fn consumes_player_supplemental(self) -> bool {
        true
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LiveSupplementalCard {
    key: CardKey,
    zone: Zone,
    consumed: bool,
}

pub fn parse_live_log_line(line: &str) -> Option<LiveLogEvent> {
    if line.starts_with("OnMatchmakingMatchFound|")
        || line.starts_with("MatchmakerManager|MatchFound|")
    {
        return Some(LiveLogEvent::MatchStarted {
            game_id: parse_field_after(line, "GameId=").map(ToOwned::to_owned),
        });
    }
    if line == "GameInputHandler|OnLeaveGame" {
        return Some(LiveLogEvent::MatchLeft);
    }
    if line.contains("ApplyGameWaitingForEndTurnChange(GameWaitingForEndTurnChange)") {
        return Some(LiveLogEvent::ResolutionStarted);
    }
    if line.contains("GameView:NotifyOfGameOver()") {
        return Some(LiveLogEvent::MatchEnded);
    }
    if line.contains("LoadVfxDef|Start") && line.contains("|DrawCard") {
        return parse_card_vfx_key(line).map(|key| LiveLogEvent::PlayerDrew {
            key: CardKey(key.to_string()),
        });
    }
    if line.contains("|HighlightCardPlayable") || line.contains("|HighlightCardUnplayable") {
        return parse_card_vfx_key(line).map(|key| LiveLogEvent::PlayerHandObserved {
            key: CardKey(key.to_string()),
        });
    }
    if line.contains("|InHandOngoing") && parse_card_vfx_key(line).is_some_and(return_hand_signal) {
        return parse_card_vfx_key(line).map(|key| LiveLogEvent::PlayerHandObserved {
            key: CardKey(key.to_string()),
        });
    }
    if line.contains("|RevealLocation") {
        return parse_location_vfx_key(line).map(|key| LiveLogEvent::LocationRevealed {
            key: key.to_string(),
        });
    }
    if line.contains("RemoteGame|OnRequest|RequestType=CubeGame.StartTurnRequest") {
        return Some(LiveLogEvent::TurnStarted);
    }
    if line.starts_with("StageCard|") {
        return parse_field_after(line, "CardDefId=").map(|key| LiveLogEvent::PlayerStaged {
            key: CardKey(key.to_string()),
            entity_id: parse_field_after(line, "CardEntityId=").map(ToOwned::to_owned),
            turn: parse_field_after(line, "Turn=").and_then(|turn| turn.parse().ok()),
        });
    }
    if line.starts_with("OnStageEntityResponse|") || line.starts_with("HandleStageEntityResponse|")
    {
        return Some(LiveLogEvent::PlayerStageAccepted {
            entity_id: parse_field_after(line, "CardEntityId = ").map(ToOwned::to_owned),
            undo: parse_field_after(line, "Undo = ").is_some_and(|value| value == "True"),
        });
    }
    if line.contains("|ResolveCardPlayed") {
        return parse_card_vfx_key(line).map(|key| LiveLogEvent::CardResolved {
            key: CardKey(key.to_string()),
        });
    }
    if line.contains("LoadVfxDef|Start") && line.contains("|ThisCardDestroyedTrigger") {
        return parse_card_vfx_key(line).map(|key| LiveLogEvent::CardDestroyedTrigger {
            key: CardKey(key.to_string()),
        });
    }
    if line.contains("LoadVfxDef|Start") && line.contains("|ThisCardDiscardedTrigger") {
        return parse_card_vfx_key(line).map(|key| LiveLogEvent::CardDiscardedTrigger {
            key: CardKey(key.to_string()),
        });
    }
    if line.contains("sfx_moonknight_discard_opp") {
        return Some(LiveLogEvent::OpponentDiscardClue);
    }
    if line.contains("sfx_") && (line.contains("_destroy_") || line.contains("destroy_card")) {
        return Some(LiveLogEvent::HiddenZoneChangeClue {
            reason: "destroy-audio".to_string(),
        });
    }
    if line.contains("|PostCardResolvedTrigger") {
        return parse_location_vfx_key(line).map(|key| LiveLogEvent::HiddenZoneChangeClue {
            reason: format!("location-post-resolve:{key}"),
        });
    }
    if line.contains("LoadVfxDef|Start")
        && line.contains("CardVfxDefs/Yondu.asset|ThisCardResolvedTrigger")
    {
        return Some(LiveLogEvent::HiddenZoneChangeClue {
            reason: "yondu-resolved".to_string(),
        });
    }
    None
}

fn live_text_card(
    prefix: &str,
    index: usize,
    key: &CardKey,
    zone: Zone,
    original_deck_candidate: bool,
    consumed_from_deck: bool,
) -> TextOverlayCard {
    TextOverlayCard {
        instance_id: CardInstanceId(format!("{prefix}:{index}:{}", key.0)),
        label: key.0.clone(),
        card_definition_key: Some(key.clone()),
        knowledge: CardKnowledge::KnownCard,
        zone,
        raw_zone: Some("Player.log".to_string()),
        original_deck_candidate,
        consumed_from_deck,
    }
}

fn parse_card_vfx_key(line: &str) -> Option<&str> {
    let start = line.find("CardVfxDefs/")? + "CardVfxDefs/".len();
    let rest = &line[start..];
    let end = rest.find(".asset")?;
    Some(&rest[..end])
}

fn parse_location_vfx_key(line: &str) -> Option<&str> {
    let start = line.find("LocationVfxDefs/")? + "LocationVfxDefs/".len();
    let rest = &line[start..];
    let end = rest.find(".asset")?;
    Some(&rest[..end])
}

fn parse_field_after<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let end = rest.find('|').unwrap_or(rest.len());
    let value = rest[..end].trim();
    (!value.is_empty()).then_some(value)
}

fn push_unique<T>(items: &mut Vec<T>, item: T)
where
    T: Eq,
{
    if !items.contains(&item) {
        items.push(item);
    }
}

fn remove_first<T>(items: &mut Vec<T>, item: &T)
where
    T: Eq,
{
    if let Some(index) = items.iter().position(|candidate| candidate == item) {
        items.remove(index);
    }
}

fn upsert_supplemental(items: &mut Vec<LiveSupplementalCard>, item: LiveSupplementalCard) {
    if let Some(existing) = items.iter_mut().find(|existing| existing.key == item.key) {
        existing.zone = item.zone;
        existing.consumed = item.consumed;
    } else {
        items.push(item);
    }
}

fn discard_returned_as(discarded: &CardKey, observed: &CardKey) -> bool {
    discarded == observed
        || matches!(
            (discarded.0.as_str(), observed.0.as_str()),
            ("Khonshu", "KhonshuWaxing")
                | ("KhonshuWaxing", "KhonshuFull")
                | ("Khonshu", "KhonshuFull")
        )
}

fn self_returning_discard(key: &CardKey) -> bool {
    matches!(key.0.as_str(), "Apocalypse" | "Scorn")
}

fn return_hand_signal(key: &str) -> bool {
    matches!(
        key,
        "KhonshuWaxing" | "KhonshuFull" | "Apocalypse" | "Scorn"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_live_log_events() {
        assert_eq!(
            parse_live_log_line(
                "<color=#9AECFF><b>GameVfxManager</b></color> | LoadVfxDef|Start|CardVfxDefs/Viv.asset|DrawCard"
            ),
            Some(LiveLogEvent::PlayerDrew {
                key: CardKey("Viv".to_string())
            })
        );
        assert_eq!(
            parse_live_log_line(
                "<color=#9AECFF><b>GameVfxManager</b></color> | LoadVfxDef|End|CardVfxDefs/Viv.asset|DrawCard"
            ),
            None
        );
        assert_eq!(
            parse_live_log_line("RemoteGame|OnRequest|RequestType=CubeGame.StartTurnRequest"),
            Some(LiveLogEvent::TurnStarted)
        );
        assert_eq!(
            parse_live_log_line(
                "CubeUnity.App.State.GameManager:ApplyGameWaitingForEndTurnChange(GameWaitingForEndTurnChange)"
            ),
            Some(LiveLogEvent::ResolutionStarted)
        );
        assert_eq!(
            parse_live_log_line("CubeUnity.App.Game.GameView:NotifyOfGameOver()"),
            Some(LiveLogEvent::MatchEnded)
        );
        assert_eq!(
            parse_live_log_line("StageCard|CardDefId=Knull|CardEntityId=18|ZoneEntityId=15|Turn=6"),
            Some(LiveLogEvent::PlayerStaged {
                key: CardKey("Knull".to_string()),
                entity_id: Some("18".to_string()),
                turn: Some(6)
            })
        );
        assert_eq!(
            parse_live_log_line(
                "<color=#9AECFF><b>GameVfxManager</b></color> | LoadVfxDef|Start|CardVfxDefs/Sentinel.asset|HighlightCardPlayable"
            ),
            Some(LiveLogEvent::PlayerHandObserved {
                key: CardKey("Sentinel".to_string())
            })
        );
        assert_eq!(
            parse_live_log_line(
                "<color=#9AECFF><b>GameVfxManager</b></color> | LoadVfxDef|Start|CardVfxDefs/BlackPanther.asset|HighlightCardUnplayable"
            ),
            Some(LiveLogEvent::PlayerHandObserved {
                key: CardKey("BlackPanther".to_string())
            })
        );
        assert_eq!(
            parse_live_log_line(
                "<color=#9AECFF><b>GameVfxManager</b></color> | LoadVfxDef|Start|LocationVfxDefs/ThePeak.asset|RevealLocation"
            ),
            Some(LiveLogEvent::LocationRevealed {
                key: "ThePeak".to_string()
            })
        );
        assert_eq!(
            parse_live_log_line(
                "OnMatchmakingMatchFound|GameId=d2460cd4-7804-455e-8ebf-e6d0193fc243|GameHostUrl=wss://example"
            ),
            Some(LiveLogEvent::MatchStarted {
                game_id: Some("d2460cd4-7804-455e-8ebf-e6d0193fc243".to_string())
            })
        );
        assert_eq!(
            parse_live_log_line(
                "OnStageEntityResponse|ClientStageRequest | CurrentState = Pending| Turn = 6| CardEntityId = 18| SourceZoneEntityId = 9| Undo = False|StageEntityResponse | Accepted = True"
            ),
            Some(LiveLogEvent::PlayerStageAccepted {
                entity_id: Some("18".to_string()),
                undo: false
            })
        );
        assert_eq!(
            parse_live_log_line(
                "<color=#9AECFF><b>GameVfxManager</b></color> | LoadVfxDef|Start|CardVfxDefs/Nova.asset|ThisCardDestroyedTrigger"
            ),
            Some(LiveLogEvent::CardDestroyedTrigger {
                key: CardKey("Nova".to_string())
            })
        );
        assert_eq!(
            parse_live_log_line(
                "<color=#9AECFF><b>GameVfxManager</b></color> | LoadVfxDef|Start|CardVfxDefs/Scorn.asset|ThisCardDiscardedTrigger"
            ),
            Some(LiveLogEvent::CardDiscardedTrigger {
                key: CardKey("Scorn".to_string())
            })
        );
        assert_eq!(
            parse_live_log_line(
                "<color=#9AECFF><b>GameVfxManager</b></color> | LoadVfxDef|Start|CardVfxDefs/Apocalypse.asset|InHandOngoing"
            ),
            Some(LiveLogEvent::PlayerHandObserved {
                key: CardKey("Apocalypse".to_string())
            })
        );
        assert_eq!(
            parse_live_log_line(
                "<color=#9AECFF><b>GameVfxManager</b></color> | LoadVfxDef|Start|CardVfxDefs/LadySif.asset|InHandOngoing"
            ),
            None
        );
        assert_eq!(
            parse_live_log_line(
                "Audio Clip for Addressable file 'sfx_moonknight_discard_opp' of Sound Group 'sfx_moonknight_discard_player' has 'Preload Audio Data' turned off"
            ),
            Some(LiveLogEvent::OpponentDiscardClue)
        );
    }

    #[test]
    fn removed_cards_are_bucketed_separately_from_destroyed_cards() {
        let mut state = LiveLogState::default();
        state.set_player_deck([
            CardKey("Brood".to_string()),
            CardKey("SilverSurfer".to_string()),
        ]);

        state.apply(LiveLogEvent::PlayerCardRemoved {
            key: CardKey("Brood".to_string()),
        });

        assert_eq!(state.player_removed, vec![CardKey("Brood".to_string())]);
        assert!(state.player_destroyed.is_empty());
        assert!(
            state
                .player_consumed_keys
                .contains(&CardKey("Brood".to_string()))
        );
    }

    #[test]
    fn discarded_cards_are_bucketed_and_marked_consumed() {
        let mut state = LiveLogState::default();
        state.set_player_deck([
            CardKey("LadySif".to_string()),
            CardKey("Apocalypse".to_string()),
        ]);

        state.apply(LiveLogEvent::PlayerDrew {
            key: CardKey("LadySif".to_string()),
        });
        state.apply(LiveLogEvent::CardDiscardedTrigger {
            key: CardKey("LadySif".to_string()),
        });

        assert_eq!(state.player_discarded, vec![CardKey("LadySif".to_string())]);
        assert!(state.player_hand_keys.is_empty());
        assert!(
            state
                .player_consumed_keys
                .contains(&CardKey("LadySif".to_string()))
        );
    }

    #[test]
    fn self_returning_discard_cards_do_not_stay_discarded() {
        let mut state = LiveLogState::default();
        state.set_player_deck([CardKey("Apocalypse".to_string())]);

        state.apply(LiveLogEvent::PlayerDrew {
            key: CardKey("Apocalypse".to_string()),
        });
        state.apply(LiveLogEvent::CardDiscardedTrigger {
            key: CardKey("Apocalypse".to_string()),
        });

        assert!(state.player_discarded.is_empty());
        assert_eq!(
            state.player_hand_keys,
            vec![CardKey("Apocalypse".to_string())]
        );
    }

    #[test]
    fn self_returning_snapshot_discard_is_scrubbed_without_erasing_other_discards() {
        let mut state = LiveLogState::default();
        state.set_player_deck([
            CardKey("Apocalypse".to_string()),
            CardKey("LadySif".to_string()),
        ]);
        state.apply(LiveLogEvent::PlayerDrew {
            key: CardKey("Apocalypse".to_string()),
        });
        state.apply(LiveLogEvent::CardDiscardedTrigger {
            key: CardKey("Apocalypse".to_string()),
        });

        let mut payload = minimal_payload();
        payload.player.discarded.push(live_text_card(
            "snapshot:discarded",
            0,
            &CardKey("Apocalypse".to_string()),
            Zone::Discarded,
            true,
            true,
        ));
        payload.player.discarded.push(live_text_card(
            "snapshot:discarded",
            1,
            &CardKey("LadySif".to_string()),
            Zone::Discarded,
            true,
            true,
        ));
        payload.player.counters.discarded = 2;

        state.apply_to_snapshot_payload(&mut payload);

        assert_eq!(payload.player.counters.discarded, 1);
        assert_eq!(payload.player.discarded[0].label, "LadySif");
    }

    #[test]
    fn khonshu_return_forms_clear_prior_discard_forms() {
        let mut state = LiveLogState::default();
        state.set_player_deck([CardKey("Khonshu".to_string())]);

        state.apply(LiveLogEvent::PlayerDrew {
            key: CardKey("Khonshu".to_string()),
        });
        state.apply(LiveLogEvent::CardDiscardedTrigger {
            key: CardKey("Khonshu".to_string()),
        });
        state.apply(LiveLogEvent::PlayerHandObserved {
            key: CardKey("KhonshuWaxing".to_string()),
        });
        state.apply(LiveLogEvent::CardDiscardedTrigger {
            key: CardKey("KhonshuWaxing".to_string()),
        });
        state.apply(LiveLogEvent::PlayerHandObserved {
            key: CardKey("KhonshuFull".to_string()),
        });

        assert!(state.player_discarded.is_empty());
        assert!(
            state
                .player_hand_keys
                .contains(&CardKey("KhonshuFull".to_string()))
        );
    }

    #[test]
    fn tracks_player_consumed_and_supplemental_cards() {
        let mut state = LiveLogState::default();
        state.set_player_deck([
            CardKey("Korg".to_string()),
            CardKey("Rockslide".to_string()),
        ]);

        assert!(state.apply(LiveLogEvent::PlayerDrew {
            key: CardKey("Korg".to_string())
        }));
        assert!(state.apply(LiveLogEvent::PlayerDrew {
            key: CardKey("Rock".to_string())
        }));

        assert!(
            state
                .player_consumed_keys
                .contains(&CardKey("Korg".to_string()))
        );
        assert_eq!(
            state.player_supplemental,
            vec![LiveSupplementalCard {
                key: CardKey("Rock".to_string()),
                zone: Zone::Hand,
                consumed: true
            }]
        );
    }

    #[test]
    fn hand_observed_non_deck_cards_are_supplemental() {
        let mut state = LiveLogState::default();
        state.set_player_deck([CardKey("Korg".to_string())]);

        state.apply(LiveLogEvent::PlayerHandObserved {
            key: CardKey("Sentinel".to_string()),
        });

        assert_eq!(
            state.player_supplemental,
            vec![LiveSupplementalCard {
                key: CardKey("Sentinel".to_string()),
                zone: Zone::Hand,
                consumed: true
            }]
        );
    }

    #[test]
    fn peak_swap_marks_prior_hand_card_away_and_tracks_incoming_supplemental() {
        let mut state = LiveLogState::default();
        state.set_player_deck([
            CardKey("Nova".to_string()),
            CardKey("Forge".to_string()),
            CardKey("Viv".to_string()),
        ]);

        state.apply(LiveLogEvent::PlayerDrew {
            key: CardKey("Nova".to_string()),
        });
        state.apply(LiveLogEvent::PlayerDrew {
            key: CardKey("Forge".to_string()),
        });
        state.apply(LiveLogEvent::LocationRevealed {
            key: "ThePeak".to_string(),
        });
        state.apply(LiveLogEvent::PlayerHandObserved {
            key: CardKey("BlackPanther".to_string()),
        });

        assert_eq!(state.player_away_keys, vec![CardKey("Nova".to_string())]);
        assert_eq!(
            state.player_supplemental,
            vec![LiveSupplementalCard {
                key: CardKey("BlackPanther".to_string()),
                zone: Zone::Hand,
                consumed: true
            }]
        );
        assert_eq!(
            state.opponent_supplemental,
            vec![LiveSupplementalCard {
                key: CardKey("Nova".to_string()),
                zone: Zone::Hand,
                consumed: false
            }]
        );
        assert!(
            state
                .player_consumed_keys
                .contains(&CardKey("Nova".to_string()))
        );
    }

    #[test]
    fn opponent_supplemental_dims_after_resolve() {
        let mut state = LiveLogState::default();
        state.opponent_supplemental.push(LiveSupplementalCard {
            key: CardKey("Nova".to_string()),
            zone: Zone::Hand,
            consumed: false,
        });

        state.apply(LiveLogEvent::CardResolved {
            key: CardKey("Nova".to_string()),
        });

        assert_eq!(
            state.opponent_supplemental,
            vec![LiveSupplementalCard {
                key: CardKey("Nova".to_string()),
                zone: Zone::Board,
                consumed: true
            }]
        );
    }

    #[test]
    fn snapshot_opponent_supplemental_in_play_is_dimmed() {
        let state = LiveLogState::default();
        let mut payload = minimal_payload();
        payload.opponent.supplemental.push(live_text_card(
            "snapshot:opponent-supplemental",
            0,
            &CardKey("WinterSoldier".to_string()),
            Zone::Board,
            false,
            false,
        ));

        state.apply_to_snapshot_payload(&mut payload);

        assert!(payload.opponent.supplemental[0].consumed_from_deck);
    }

    #[test]
    fn snapshot_opponent_supplemental_in_hand_stays_bright() {
        let state = LiveLogState::default();
        let mut payload = minimal_payload();
        payload.opponent.supplemental.push(live_text_card(
            "snapshot:opponent-supplemental",
            0,
            &CardKey("WinterSoldier".to_string()),
            Zone::Hand,
            false,
            false,
        ));

        state.apply_to_snapshot_payload(&mut payload);

        assert!(!payload.opponent.supplemental[0].consumed_from_deck);
    }

    #[test]
    fn opponent_resolved_cards_are_stably_recorded_once() {
        let mut state = LiveLogState::default();
        state.apply(LiveLogEvent::CardResolved {
            key: CardKey("MistyKnight".to_string()),
        });
        state.apply(LiveLogEvent::CardResolved {
            key: CardKey("MistyKnight".to_string()),
        });

        assert_eq!(
            state.opponent_known,
            vec![CardKey("MistyKnight".to_string())]
        );
        assert!(
            state
                .opponent_consumed_keys
                .contains(&CardKey("MistyKnight".to_string()))
        );
    }

    #[test]
    fn opponent_discarded_cards_are_projected_to_opponent_discard_bucket() {
        let mut state = LiveLogState::default();
        state.apply(LiveLogEvent::OpponentCardDiscarded {
            key: CardKey("ArnimZola".to_string()),
        });

        let mut payload = TextOverlayPayload {
            schema_version: 1,
            lifecycle: domain::MatchLifecycle::Unknown,
            turn: None,
            total_turns: None,
            player: state_reader::TextOverlayPanel {
                participant: domain::Participant::Player,
                title: "Player".to_string(),
                deck_slots: Vec::new(),
                supplemental: Vec::new(),
                destroyed: Vec::new(),
                discarded: Vec::new(),
                removed: Vec::new(),
                unknown_transition: Vec::new(),
                counters: state_reader::TextOverlayCounters {
                    deck: 0,
                    hand: 0,
                    board: 0,
                    destroyed: 0,
                    discarded: 0,
                    removed: 0,
                    supplemental: 0,
                    unknown_transition: 0,
                },
            },
            opponent: state_reader::TextOverlayPanel {
                participant: domain::Participant::Opponent,
                title: "Opponent".to_string(),
                deck_slots: (0..12)
                    .map(|slot_index| state_reader::TextOverlaySlot {
                        slot_index,
                        state: TextOverlaySlotState::Unknown,
                        card: None,
                    })
                    .collect(),
                supplemental: Vec::new(),
                destroyed: Vec::new(),
                discarded: Vec::new(),
                removed: Vec::new(),
                unknown_transition: Vec::new(),
                counters: state_reader::TextOverlayCounters {
                    deck: 0,
                    hand: 0,
                    board: 0,
                    destroyed: 0,
                    discarded: 0,
                    removed: 0,
                    supplemental: 0,
                    unknown_transition: 0,
                },
            },
            warnings: Vec::new(),
        };

        state.apply_to_payload(&mut payload);

        assert_eq!(payload.opponent.counters.discarded, 1);
        assert_eq!(payload.opponent.discarded[0].label, "ArnimZola");
    }

    #[test]
    #[ignore = "live counter inference disabled while testing JSON-authoritative refresh"]
    fn live_counters_track_ordinary_opponent_turn_draws_and_plays() {
        let mut state = LiveLogState::default();
        let mut payload = minimal_payload();
        payload.opponent.counters.deck = 8;
        payload.opponent.counters.hand = 4;
        state.seed_counters_from_payload(&payload);

        state.apply(LiveLogEvent::TurnStarted);
        state.apply_to_payload(&mut payload);
        assert_eq!(payload.opponent.counters.deck, 8);
        assert_eq!(payload.opponent.counters.hand, 4);

        state.apply(LiveLogEvent::TurnStarted);
        state.apply(LiveLogEvent::CardResolved {
            key: CardKey("MistyKnight".to_string()),
        });
        state.apply_to_payload(&mut payload);

        assert_eq!(payload.opponent.counters.deck, 7);
        assert_eq!(payload.opponent.counters.hand, 4);
    }

    #[test]
    #[ignore = "live counter inference disabled while testing JSON-authoritative refresh"]
    fn live_counters_track_player_draw_play_and_returning_discard() {
        let mut state = LiveLogState::default();
        state.set_player_deck([
            CardKey("Blade".to_string()),
            CardKey("Apocalypse".to_string()),
        ]);
        let mut payload = minimal_payload();
        payload.player.counters.deck = 8;
        payload.player.counters.hand = 4;
        state.seed_counters_from_payload(&payload);

        state.apply(LiveLogEvent::PlayerDrew {
            key: CardKey("Blade".to_string()),
        });
        state.apply(LiveLogEvent::PlayerStaged {
            key: CardKey("Blade".to_string()),
            entity_id: Some("42".to_string()),
            turn: Some(2),
        });
        state.apply(LiveLogEvent::CardDiscardedTrigger {
            key: CardKey("Apocalypse".to_string()),
        });
        state.apply_to_payload(&mut payload);

        assert_eq!(payload.player.counters.deck, 7);
        assert_eq!(payload.player.counters.hand, 4);
    }

    #[test]
    fn snapshot_discard_bucket_survives_returning_live_discard() {
        let mut state = LiveLogState::default();
        state.set_player_deck([
            CardKey("LadySif".to_string()),
            CardKey("Apocalypse".to_string()),
        ]);
        state.apply(LiveLogEvent::PlayerDrew {
            key: CardKey("Apocalypse".to_string()),
        });
        state.apply(LiveLogEvent::CardDiscardedTrigger {
            key: CardKey("Apocalypse".to_string()),
        });

        let mut payload = minimal_payload();
        payload.player.discarded.push(live_text_card(
            "snapshot:discarded",
            0,
            &CardKey("LadySif".to_string()),
            Zone::Discarded,
            true,
            true,
        ));
        payload.player.counters.discarded = 1;

        state.apply_to_snapshot_payload(&mut payload);

        assert_eq!(payload.player.counters.discarded, 1);
        assert_eq!(payload.player.discarded[0].label, "LadySif");
    }

    #[test]
    fn log_only_update_preserves_existing_snapshot_discard_bucket() {
        let mut state = LiveLogState::default();
        state.apply(LiveLogEvent::CardDiscardedTrigger {
            key: CardKey("Scorn".to_string()),
        });

        let mut payload = minimal_payload();
        payload.player.discarded.push(live_text_card(
            "snapshot:discarded",
            0,
            &CardKey("Gambit".to_string()),
            Zone::Discarded,
            true,
            true,
        ));
        payload.player.counters.discarded = 1;

        state.apply_to_payload(&mut payload);

        assert_eq!(payload.player.counters.discarded, 1);
        assert_eq!(payload.player.discarded[0].label, "Gambit");
    }

    fn minimal_payload() -> TextOverlayPayload {
        TextOverlayPayload {
            schema_version: 1,
            lifecycle: domain::MatchLifecycle::Unknown,
            turn: None,
            total_turns: None,
            player: state_reader::TextOverlayPanel {
                participant: domain::Participant::Player,
                title: "Player".to_string(),
                deck_slots: Vec::new(),
                supplemental: Vec::new(),
                destroyed: Vec::new(),
                discarded: Vec::new(),
                removed: Vec::new(),
                unknown_transition: Vec::new(),
                counters: zero_counters(),
            },
            opponent: state_reader::TextOverlayPanel {
                participant: domain::Participant::Opponent,
                title: "Opponent".to_string(),
                deck_slots: (0..12)
                    .map(|slot_index| state_reader::TextOverlaySlot {
                        slot_index,
                        state: TextOverlaySlotState::Unknown,
                        card: None,
                    })
                    .collect(),
                supplemental: Vec::new(),
                destroyed: Vec::new(),
                discarded: Vec::new(),
                removed: Vec::new(),
                unknown_transition: Vec::new(),
                counters: zero_counters(),
            },
            warnings: Vec::new(),
        }
    }

    fn zero_counters() -> state_reader::TextOverlayCounters {
        state_reader::TextOverlayCounters {
            deck: 0,
            hand: 0,
            board: 0,
            destroyed: 0,
            discarded: 0,
            removed: 0,
            supplemental: 0,
            unknown_transition: 0,
        }
    }
}
