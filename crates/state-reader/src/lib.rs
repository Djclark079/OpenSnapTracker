//! Robust read primitives for Marvel Snap state snapshots.
//!
//! This crate reads JSON snapshots as externally-owned files: they may be
//! replaced or rewritten while the game is running. It retries boundedly on
//! transient parse failures and never repairs malformed JSON.

use domain::{
    CardInstance, CardInstanceId, CardKey, CardKnowledge, CardOrigin, MatchEvent, MatchLifecycle,
    Participant, Provenance, RemovalReason, Zone,
};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, fs, io, path::Path, thread, time::Duration};
use thiserror::Error;
use time::OffsetDateTime;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadOptions {
    pub max_attempts: usize,
    pub initial_backoff: Duration,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_backoff: Duration::from_millis(25),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RawSnapshot {
    pub source_filename: String,
    pub raw_text: String,
    pub parsed: serde_json::Value,
    pub sha256: String,
    pub byte_len: usize,
    pub attempts: usize,
    pub captured_at: OffsetDateTime,
}

#[derive(Debug, Error)]
pub enum SnapshotReadError {
    #[error("could not read {path}: {source}")]
    Io { path: String, source: io::Error },
    #[error("snapshot {path} was malformed after {attempts} attempts: {message}")]
    Malformed {
        path: String,
        attempts: usize,
        message: String,
    },
    #[error("read options must allow at least one attempt")]
    InvalidOptions,
}

pub fn read_json_snapshot(
    path: impl AsRef<Path>,
    options: ReadOptions,
) -> Result<RawSnapshot, SnapshotReadError> {
    if options.max_attempts == 0 {
        return Err(SnapshotReadError::InvalidOptions);
    }

    let path = path.as_ref();
    let mut backoff = options.initial_backoff;
    let mut last_parse_error = None;

    for attempt in 1..=options.max_attempts {
        let raw_text = fs::read_to_string(path).map_err(|source| SnapshotReadError::Io {
            path: path.display().to_string(),
            source,
        })?;

        let json_text = raw_text.strip_prefix('\u{feff}').unwrap_or(&raw_text);
        match serde_json::from_str::<serde_json::Value>(json_text) {
            Ok(parsed) => {
                let sha256 = sha256_hex(raw_text.as_bytes());
                return Ok(RawSnapshot {
                    source_filename: path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("snapshot.json")
                        .to_string(),
                    byte_len: raw_text.len(),
                    raw_text,
                    parsed,
                    sha256,
                    attempts: attempt,
                    captured_at: OffsetDateTime::now_utc(),
                });
            }
            Err(error) => {
                last_parse_error = Some(error.to_string());
                if attempt < options.max_attempts {
                    thread::sleep(backoff);
                    backoff = backoff.saturating_mul(2);
                }
            }
        }
    }

    Err(SnapshotReadError::Malformed {
        path: path.display().to_string(),
        attempts: options.max_attempts,
        message: last_parse_error.unwrap_or_else(|| "unknown parse error".to_string()),
    })
}

#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotObservation {
    pub application_version: Option<String>,
    pub lifecycle: MatchLifecycle,
    pub turn: Option<i64>,
    pub total_turns: Option<i64>,
    pub local_player_entity_id: Option<i64>,
    pub enemy_player_entity_id: Option<i64>,
    pub game_mode_type: Option<String>,
    pub client_result_present: bool,
    pub battle_result_present: bool,
    pub client_cards_drawn_count: usize,
    pub client_cards_played_count: usize,
    pub client_stage_request_count: usize,
    pub players: Vec<PlayerObservation>,
    pub cards: Vec<CardObservation>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerObservation {
    pub participant: Participant,
    pub entity_id: Option<i64>,
    pub zones: Vec<ZoneObservation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZoneObservation {
    pub label: &'static str,
    pub raw_zone: Option<String>,
    pub domain_zone: Zone,
    pub card_entity_ids: Vec<i64>,
    pub known_count: usize,
    pub hidden_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CardObservation {
    pub entity_id: i64,
    pub owner: Participant,
    pub card_definition_key: Option<CardKey>,
    pub raw_zone: Option<String>,
    pub domain_zone: Zone,
    pub previous_raw_zone: Option<String>,
    pub previous_domain_zone: Option<Zone>,
    pub zone_position: Option<i64>,
    pub started_in_deck_entity_id: Option<i64>,
}

impl CardObservation {
    #[must_use]
    pub fn is_known(&self) -> bool {
        self.card_definition_key.is_some()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObservedTransition {
    CardAppeared {
        entity_id: i64,
        owner: Participant,
        card_definition_key: Option<CardKey>,
        raw_zone: Option<String>,
    },
    CardDefinitionRevealed {
        entity_id: i64,
        card_definition_key: CardKey,
    },
    CardZoneChanged {
        entity_id: i64,
        from_raw_zone: Option<String>,
        to_raw_zone: Option<String>,
        from_domain_zone: Zone,
        to_domain_zone: Zone,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationInput<'a> {
    pub previous: Option<&'a SnapshotObservation>,
    pub current: &'a SnapshotObservation,
    pub snapshot_version: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverlayProjection {
    pub lifecycle: MatchLifecycle,
    pub turn: Option<i64>,
    pub total_turns: Option<i64>,
    pub player: ParticipantOverlayProjection,
    pub opponent: ParticipantOverlayProjection,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParticipantOverlayProjection {
    pub participant: Participant,
    pub deck_count: usize,
    pub hand_count: usize,
    pub board_count: usize,
    pub destroyed_count: usize,
    pub discarded_count: usize,
    pub removed_count: usize,
    pub unknown_transition_count: usize,
    pub cards: Vec<OverlayCardProjection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverlayCardProjection {
    pub instance_id: CardInstanceId,
    pub card_definition_key: Option<CardKey>,
    pub knowledge: CardKnowledge,
    pub zone: Zone,
    pub raw_zone: Option<String>,
    pub original_deck_candidate: bool,
    pub consumed_from_deck: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OverlayProjector {
    classified_zones: HashMap<CardInstanceId, Zone>,
}

impl OverlayProjector {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn project(
        &mut self,
        observation: &SnapshotObservation,
        events: &[MatchEvent],
    ) -> OverlayProjection {
        self.apply_events(events);
        project_overlay_with_classifications(observation, &self.classified_zones)
    }

    fn apply_events(&mut self, events: &[MatchEvent]) {
        for event in events {
            match event {
                MatchEvent::CardDestroyed { card } => {
                    self.classified_zones.insert(card.clone(), Zone::Destroyed);
                }
                MatchEvent::CardDiscarded { card } => {
                    self.classified_zones.insert(card.clone(), Zone::Discarded);
                }
                MatchEvent::CardRemoved { card, reason } => {
                    self.classified_zones
                        .insert(card.clone(), zone_for_removal_reason(*reason));
                }
                MatchEvent::CardReturned { card, .. }
                | MatchEvent::CardDrawn { card }
                | MatchEvent::CardPlayed { card } => {
                    self.classified_zones.remove(card);
                }
                _ => {}
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum NormalizeError {
    #[error("snapshot does not contain /RemoteGame/GameState")]
    MissingGameState,
}

pub fn observe_game_state(
    value: &serde_json::Value,
) -> Result<SnapshotObservation, NormalizeError> {
    let remote = value
        .pointer("/RemoteGame")
        .ok_or(NormalizeError::MissingGameState)?;
    let game_state = remote
        .pointer("/GameState")
        .ok_or(NormalizeError::MissingGameState)?;
    let client_info = remote
        .pointer("/ClientPlayerInfo")
        .unwrap_or(&serde_json::Value::Null);
    let index = JsonNetIndex::new(value);

    let local_player_entity_id = remote
        .pointer("/ClientGameInfo/LocalPlayerEntityId")
        .and_then(serde_json::Value::as_i64);
    let enemy_player_entity_id = remote
        .pointer("/ClientGameInfo/EnemyPlayerEntityId")
        .and_then(serde_json::Value::as_i64);
    let client_result_present = game_state.get("ClientResultMessage").is_some();
    let battle_result_present = game_state
        .pointer("/GameMode/Data/ClientBattleResultMessage")
        .is_some();

    let mut players = Vec::new();
    let mut participant_by_entity_id = HashMap::new();
    if let Some(raw_players) = game_state
        .get("_players")
        .and_then(serde_json::Value::as_array)
    {
        for raw_player in raw_players {
            let player = index.resolve(raw_player);
            let entity_id = player.get("EntityId").and_then(serde_json::Value::as_i64);
            let participant =
                participant_for_entity(entity_id, local_player_entity_id, enemy_player_entity_id);
            if let Some(entity_id) = entity_id {
                participant_by_entity_id.insert(entity_id, participant);
            }

            players.push(PlayerObservation {
                participant,
                entity_id,
                zones: ["Deck", "Hand", "Graveyard", "Banished"]
                    .into_iter()
                    .map(|label| observe_zone(label, player.get(label), &index))
                    .collect(),
            });
        }
    }

    let mut cards = collect_card_observations(value, &index, &participant_by_entity_id);
    cards.sort_by_key(|card| card.entity_id);
    cards.dedup_by_key(|card| card.entity_id);

    let mut warnings = Vec::new();
    if cards
        .iter()
        .any(|card| card.raw_zone.as_deref() == Some("Graveyard"))
    {
        warnings.push(
            "raw Graveyard zone observed; unusual paths require transition context".to_string(),
        );
    }

    Ok(SnapshotObservation {
        application_version: value
            .get("ApplicationVersion")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        lifecycle: infer_lifecycle(game_state, client_result_present, !players.is_empty()),
        turn: game_state.get("Turn").and_then(serde_json::Value::as_i64),
        total_turns: game_state
            .get("TotalTurns")
            .and_then(serde_json::Value::as_i64),
        local_player_entity_id,
        enemy_player_entity_id,
        game_mode_type: game_state
            .pointer("/GameMode/$type")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        client_result_present,
        battle_result_present,
        client_cards_drawn_count: array_len(client_info.get("CardsDrawn")),
        client_cards_played_count: array_len(client_info.get("CardsPlayed")),
        client_stage_request_count: array_len(client_info.get("ClientStageRequests")),
        players,
        cards,
        warnings,
    })
}

#[must_use]
pub fn diff_observations(
    previous: &SnapshotObservation,
    current: &SnapshotObservation,
) -> Vec<ObservedTransition> {
    let previous_cards: HashMap<i64, &CardObservation> = previous
        .cards
        .iter()
        .map(|card| (card.entity_id, card))
        .collect();
    let mut transitions = Vec::new();

    for card in &current.cards {
        let Some(previous_card) = previous_cards.get(&card.entity_id) else {
            transitions.push(ObservedTransition::CardAppeared {
                entity_id: card.entity_id,
                owner: card.owner,
                card_definition_key: card.card_definition_key.clone(),
                raw_zone: card.raw_zone.clone(),
            });
            continue;
        };

        if previous_card.card_definition_key.is_none()
            && let Some(card_definition_key) = &card.card_definition_key
        {
            transitions.push(ObservedTransition::CardDefinitionRevealed {
                entity_id: card.entity_id,
                card_definition_key: card_definition_key.clone(),
            });
        }

        if previous_card.raw_zone != card.raw_zone {
            transitions.push(ObservedTransition::CardZoneChanged {
                entity_id: card.entity_id,
                from_raw_zone: previous_card.raw_zone.clone(),
                to_raw_zone: card.raw_zone.clone(),
                from_domain_zone: previous_card.domain_zone,
                to_domain_zone: card.domain_zone,
            });
        }
    }

    transitions
}

#[must_use]
pub fn reconcile_observation(input: ReconciliationInput<'_>) -> Vec<MatchEvent> {
    let mut events = Vec::new();
    let previous = input.previous.filter(|previous| {
        !should_reset_reconciliation(previous.lifecycle, input.current.lifecycle)
    });

    if previous.is_none() && input.current.lifecycle == MatchLifecycle::InMatch {
        events.push(MatchEvent::MatchStarted {
            snapshot_version: input.snapshot_version.clone(),
        });
    }

    for warning in &input.current.warnings {
        events.push(MatchEvent::SnapshotParseWarning {
            message: warning.clone(),
        });
    }

    match previous {
        None => {
            for card in &input.current.cards {
                events.push(MatchEvent::CardInstanceObserved {
                    card: card_instance_from_observation(card),
                });
            }
        }
        Some(previous) => {
            if previous.lifecycle != MatchLifecycle::MatchEnded
                && input.current.lifecycle == MatchLifecycle::MatchEnded
            {
                events.push(MatchEvent::MatchEnded);
            }

            let current_by_entity_id: HashMap<i64, &CardObservation> = input
                .current
                .cards
                .iter()
                .map(|card| (card.entity_id, card))
                .collect();

            for transition in diff_observations(previous, input.current) {
                match transition {
                    ObservedTransition::CardAppeared { entity_id, .. } => {
                        if let Some(card) = current_by_entity_id.get(&entity_id) {
                            events.push(MatchEvent::CardInstanceObserved {
                                card: card_instance_from_observation(card),
                            });
                            if card.started_in_deck_entity_id.is_none() {
                                events.push(MatchEvent::CardGenerated {
                                    card: card_instance_id(entity_id),
                                    origin: CardOrigin::UnknownExternal,
                                });
                            }
                        }
                    }
                    ObservedTransition::CardDefinitionRevealed { entity_id, .. } => {
                        events.push(MatchEvent::CardRevealed {
                            card: card_instance_id(entity_id),
                        });
                    }
                    ObservedTransition::CardZoneChanged {
                        entity_id,
                        from_domain_zone,
                        to_domain_zone,
                        from_raw_zone,
                        to_raw_zone,
                    } => match (from_domain_zone, to_domain_zone) {
                        (Zone::Deck, Zone::Hand) => events.push(MatchEvent::CardDrawn {
                            card: card_instance_id(entity_id),
                        }),
                        (Zone::Hand, Zone::Board) => events.push(MatchEvent::CardPlayed {
                            card: card_instance_id(entity_id),
                        }),
                        (Zone::Hand, Zone::UnknownTransition)
                            if to_raw_zone.as_deref() == Some("Graveyard") =>
                        {
                            events.push(MatchEvent::CardDiscarded {
                                card: card_instance_id(entity_id),
                            });
                        }
                        (Zone::Board, Zone::UnknownTransition)
                            if to_raw_zone.as_deref() == Some("Graveyard") =>
                        {
                            events.push(MatchEvent::CardDestroyed {
                                card: card_instance_id(entity_id),
                            });
                        }
                        (_, Zone::UnknownTransition) => {
                            events.push(MatchEvent::UnknownTransitionObserved {
                                card: Some(card_instance_id(entity_id)),
                                details: serde_json::json!({
                                    "from_raw_zone": from_raw_zone,
                                    "to_raw_zone": to_raw_zone,
                                    "reason": "raw zone requires reconciliation context"
                                }),
                            });
                        }
                        _ => events.push(MatchEvent::UnknownTransitionObserved {
                            card: Some(card_instance_id(entity_id)),
                            details: serde_json::json!({
                                "from_raw_zone": from_raw_zone,
                                "to_raw_zone": to_raw_zone,
                                "from_domain_zone": format!("{from_domain_zone:?}"),
                                "to_domain_zone": format!("{to_domain_zone:?}")
                            }),
                        }),
                    },
                }
            }
        }
    }

    events
}

fn should_reset_reconciliation(previous: MatchLifecycle, current: MatchLifecycle) -> bool {
    matches!(
        previous,
        MatchLifecycle::Unknown | MatchLifecycle::MatchEnded
    ) && current == MatchLifecycle::InMatch
}

#[must_use]
pub fn project_overlay(observation: &SnapshotObservation) -> OverlayProjection {
    project_overlay_with_classifications(observation, &HashMap::new())
}

fn project_overlay_with_classifications(
    observation: &SnapshotObservation,
    classified_zones: &HashMap<CardInstanceId, Zone>,
) -> OverlayProjection {
    OverlayProjection {
        lifecycle: observation.lifecycle,
        turn: observation.turn,
        total_turns: observation.total_turns,
        player: project_participant(observation, Participant::Player, classified_zones),
        opponent: project_participant(observation, Participant::Opponent, classified_zones),
        warnings: observation.warnings.clone(),
    }
}

fn project_participant(
    observation: &SnapshotObservation,
    participant: Participant,
    classified_zones: &HashMap<CardInstanceId, Zone>,
) -> ParticipantOverlayProjection {
    let deck_count = zone_count(observation, participant, "Deck");
    let hand_count = zone_count(observation, participant, "Hand");
    let mut cards = observation
        .cards
        .iter()
        .filter(|card| card.owner == participant)
        .map(|card| {
            let original_deck_candidate = card.started_in_deck_entity_id.is_some();
            let instance_id = card_instance_id(card.entity_id);
            let zone = effective_projection_zone(card, &instance_id, classified_zones);
            OverlayCardProjection {
                instance_id,
                card_definition_key: card.card_definition_key.clone(),
                knowledge: if card.is_known() {
                    CardKnowledge::KnownCard
                } else {
                    CardKnowledge::UnknownCard
                },
                zone,
                raw_zone: card.raw_zone.clone(),
                original_deck_candidate,
                consumed_from_deck: original_deck_candidate && zone != Zone::Deck,
            }
        })
        .collect::<Vec<_>>();
    cards.sort_by(|left, right| {
        zone_sort_key(left.zone)
            .cmp(&zone_sort_key(right.zone))
            .then_with(|| left.instance_id.0.cmp(&right.instance_id.0))
    });

    ParticipantOverlayProjection {
        participant,
        deck_count,
        hand_count,
        board_count: cards.iter().filter(|card| card.zone == Zone::Board).count(),
        destroyed_count: cards
            .iter()
            .filter(|card| card.zone == Zone::Destroyed)
            .count(),
        discarded_count: cards
            .iter()
            .filter(|card| card.zone == Zone::Discarded)
            .count(),
        removed_count: cards
            .iter()
            .filter(|card| card.zone == Zone::RemovedConfirmed)
            .count(),
        unknown_transition_count: cards
            .iter()
            .filter(|card| card.zone == Zone::UnknownTransition)
            .count(),
        cards,
    }
}

fn effective_projection_zone(
    card: &CardObservation,
    instance_id: &CardInstanceId,
    classified_zones: &HashMap<CardInstanceId, Zone>,
) -> Zone {
    if card.raw_zone.as_deref() == Some("Graveyard")
        && let Some(zone) = classified_zones.get(instance_id).copied()
    {
        return zone;
    }
    card.domain_zone
}

fn zone_for_removal_reason(reason: RemovalReason) -> Zone {
    match reason {
        RemovalReason::Destroyed => Zone::Destroyed,
        RemovalReason::Discarded => Zone::Discarded,
        RemovalReason::RemovedConfirmed => Zone::RemovedConfirmed,
        RemovalReason::Transformed => Zone::Transformed,
        RemovalReason::Merged => Zone::Merged,
        RemovalReason::Returned => Zone::Returned,
        RemovalReason::UnknownTransition => Zone::UnknownTransition,
    }
}

fn zone_count(
    observation: &SnapshotObservation,
    participant: Participant,
    label: &'static str,
) -> usize {
    observation
        .players
        .iter()
        .find(|player| player.participant == participant)
        .and_then(|player| player.zones.iter().find(|zone| zone.label == label))
        .map_or(0, |zone| zone.card_entity_ids.len())
}

fn zone_sort_key(zone: Zone) -> u8 {
    match zone {
        Zone::Deck => 0,
        Zone::Hand => 1,
        Zone::Board => 2,
        Zone::Destroyed => 3,
        Zone::Discarded => 4,
        Zone::RemovedConfirmed => 5,
        Zone::Transformed => 6,
        Zone::Merged => 7,
        Zone::Returned => 8,
        Zone::UnknownTransition => 9,
        Zone::Unknown => 10,
    }
}

fn card_instance_from_observation(card: &CardObservation) -> CardInstance {
    CardInstance {
        internal_instance_id: card_instance_id(card.entity_id),
        external_game_instance_id: Some(card.entity_id.to_string()),
        card_definition_key: card.card_definition_key.clone(),
        owner: card.owner,
        controller: card.owner,
        origin: if card.started_in_deck_entity_id.is_some() {
            CardOrigin::OriginalDeck
        } else {
            CardOrigin::Unknown
        },
        current_zone: card.domain_zone,
        previous_zone: card.previous_domain_zone,
        original_deck_slot: None,
        knowledge: if card.is_known() {
            CardKnowledge::KnownCard
        } else {
            CardKnowledge::UnknownCard
        },
        provenance: Provenance::observed(),
    }
}

fn card_instance_id(entity_id: i64) -> CardInstanceId {
    CardInstanceId(format!("game:{entity_id}"))
}

fn infer_lifecycle(
    game_state: &serde_json::Value,
    client_result_present: bool,
    has_players: bool,
) -> MatchLifecycle {
    if client_result_present {
        MatchLifecycle::MatchEnded
    } else if game_state.get("Turn").is_some() && has_players {
        MatchLifecycle::InMatch
    } else {
        MatchLifecycle::Unknown
    }
}

fn observe_zone(
    label: &'static str,
    zone: Option<&serde_json::Value>,
    index: &JsonNetIndex<'_>,
) -> ZoneObservation {
    let zone = index.resolve(zone.unwrap_or(&serde_json::Value::Null));
    let raw_zone = zone
        .get("ZoneId")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    let mut card_entity_ids = Vec::new();
    let mut known_count = 0;
    let mut hidden_count = 0;

    if let Some(cards) = zone.get("_cards").and_then(serde_json::Value::as_array) {
        for card_ref in cards {
            let card = index.resolve(card_ref);
            if let Some(entity_id) = card.get("EntityId").and_then(serde_json::Value::as_i64) {
                card_entity_ids.push(entity_id);
                if card
                    .get("CardDefId")
                    .and_then(serde_json::Value::as_str)
                    .is_some()
                {
                    known_count += 1;
                } else {
                    hidden_count += 1;
                }
            }
        }
    }

    ZoneObservation {
        label,
        domain_zone: map_raw_zone(raw_zone.as_deref()),
        raw_zone,
        card_entity_ids,
        known_count,
        hidden_count,
    }
}

fn collect_card_observations(
    value: &serde_json::Value,
    index: &JsonNetIndex<'_>,
    participant_by_entity_id: &HashMap<i64, Participant>,
) -> Vec<CardObservation> {
    let mut cards = Vec::new();
    collect_card_observations_inner(value, index, participant_by_entity_id, &mut cards);
    cards
}

fn collect_card_observations_inner(
    value: &serde_json::Value,
    index: &JsonNetIndex<'_>,
    participant_by_entity_id: &HashMap<i64, Participant>,
    cards: &mut Vec<CardObservation>,
) {
    match value {
        serde_json::Value::Object(map) => {
            if map
                .get("$type")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|type_name| type_name.starts_with("CubeGame.Card,"))
                && let Some(entity_id) = map.get("EntityId").and_then(serde_json::Value::as_i64)
            {
                let owner = index.resolve(map.get("Owner").unwrap_or(&serde_json::Value::Null));
                let owner_entity_id = owner.get("EntityId").and_then(serde_json::Value::as_i64);
                let owner = owner_entity_id
                    .and_then(|entity_id| participant_by_entity_id.get(&entity_id).copied())
                    .unwrap_or(Participant::Unknown);
                let raw_zone = resolved_zone_id(map.get("_zone"), index);
                let previous_raw_zone = resolved_zone_id(map.get("_previousZone"), index);

                cards.push(CardObservation {
                    entity_id,
                    owner,
                    card_definition_key: map
                        .get("CardDefId")
                        .and_then(serde_json::Value::as_str)
                        .map(|key| CardKey(key.to_string())),
                    domain_zone: map_raw_zone(raw_zone.as_deref()),
                    raw_zone,
                    previous_domain_zone: previous_raw_zone
                        .as_deref()
                        .map(|raw_zone| map_raw_zone(Some(raw_zone))),
                    previous_raw_zone,
                    zone_position: map.get("ZonePosition").and_then(serde_json::Value::as_i64),
                    started_in_deck_entity_id: map
                        .get("StartedInDeckEntityId")
                        .and_then(serde_json::Value::as_i64),
                });
            }

            for child in map.values() {
                collect_card_observations_inner(child, index, participant_by_entity_id, cards);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                collect_card_observations_inner(child, index, participant_by_entity_id, cards);
            }
        }
        _ => {}
    }
}

fn resolved_zone_id(value: Option<&serde_json::Value>, index: &JsonNetIndex<'_>) -> Option<String> {
    index
        .resolve(value.unwrap_or(&serde_json::Value::Null))
        .get("ZoneId")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

fn participant_for_entity(
    entity_id: Option<i64>,
    local_player_entity_id: Option<i64>,
    enemy_player_entity_id: Option<i64>,
) -> Participant {
    match entity_id {
        Some(entity_id) if Some(entity_id) == local_player_entity_id => Participant::Player,
        Some(entity_id) if Some(entity_id) == enemy_player_entity_id => Participant::Opponent,
        _ => Participant::Unknown,
    }
}

fn map_raw_zone(raw_zone: Option<&str>) -> Zone {
    match raw_zone {
        Some("Deck") => Zone::Deck,
        Some("Hand") => Zone::Hand,
        Some("Location") => Zone::Board,
        Some("Banished") => Zone::RemovedConfirmed,
        Some("Graveyard") => Zone::UnknownTransition,
        _ => Zone::Unknown,
    }
}

fn array_len(value: Option<&serde_json::Value>) -> usize {
    value
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len)
}

#[derive(Debug)]
struct JsonNetIndex<'a> {
    values_by_id: HashMap<&'a str, &'a serde_json::Value>,
}

impl<'a> JsonNetIndex<'a> {
    fn new(root: &'a serde_json::Value) -> Self {
        let mut index = Self {
            values_by_id: HashMap::new(),
        };
        index.collect(root);
        index
    }

    fn resolve<'b>(&'b self, value: &'b serde_json::Value) -> &'b serde_json::Value
    where
        'a: 'b,
    {
        value
            .get("$ref")
            .and_then(serde_json::Value::as_str)
            .and_then(|id| self.values_by_id.get(id).copied())
            .unwrap_or(value)
    }

    fn collect(&mut self, value: &'a serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(id) = map.get("$id").and_then(serde_json::Value::as_str) {
                    self.values_by_id.insert(id, value);
                }
                for child in map.values() {
                    self.collect(child);
                }
            }
            serde_json::Value::Array(items) => {
                for child in items {
                    self.collect(child);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::Duration};
    use tempfile::tempdir;

    #[test]
    fn reads_valid_json_and_preserves_unknown_fields() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("GameState.json");
        fs::write(&path, r#"{"RemoteGame":{"unexpected":true},"unknown":42}"#)
            .expect("write fixture");

        let snapshot = read_json_snapshot(
            &path,
            ReadOptions {
                max_attempts: 1,
                initial_backoff: Duration::ZERO,
            },
        )
        .expect("snapshot reads");

        assert_eq!(snapshot.source_filename, "GameState.json");
        assert_eq!(snapshot.parsed["RemoteGame"]["unexpected"], true);
        assert_eq!(snapshot.parsed["unknown"], 42);
        assert_eq!(snapshot.sha256, sha256_hex(snapshot.raw_text.as_bytes()));
    }

    #[test]
    fn malformed_json_is_not_repaired() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("GameState.json");
        fs::write(&path, r#"{"RemoteGame":"#).expect("write fixture");

        let err = read_json_snapshot(
            &path,
            ReadOptions {
                max_attempts: 2,
                initial_backoff: Duration::ZERO,
            },
        )
        .expect_err("malformed json should fail");

        assert!(matches!(
            err,
            SnapshotReadError::Malformed { attempts: 2, .. }
        ));
    }

    #[test]
    fn accepts_utf8_bom_without_repairing_json() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("GameState.json");
        fs::write(&path, "\u{feff}{\"RemoteGame\":{\"GameState\":{}}}").expect("write fixture");

        let snapshot = read_json_snapshot(
            &path,
            ReadOptions {
                max_attempts: 1,
                initial_backoff: Duration::ZERO,
            },
        )
        .expect("snapshot reads");

        assert!(snapshot.raw_text.starts_with('\u{feff}'));
        assert_eq!(
            snapshot.parsed["RemoteGame"]["GameState"],
            serde_json::json!({})
        );
    }

    #[test]
    fn eventually_consistent_read_can_succeed() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("GameState.json");
        fs::write(&path, r#"{"RemoteGame":"#).expect("write partial");

        let writer_path = path.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            fs::write(writer_path, r#"{"RemoteGame":{"GameState":{}}}"#).expect("write complete");
        });

        let snapshot = read_json_snapshot(
            &path,
            ReadOptions {
                max_attempts: 5,
                initial_backoff: Duration::from_millis(10),
            },
        )
        .expect("eventually reads");
        handle.join().expect("writer joins");

        assert_eq!(
            snapshot.parsed["RemoteGame"]["GameState"],
            serde_json::json!({})
        );
        assert!(snapshot.attempts > 1);
    }

    #[test]
    fn observes_sanitized_active_turn_with_resolved_references() {
        let value: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-active-turn.json"
        ))
        .expect("fixture parses");

        let observation = observe_game_state(&value).expect("observes game state");

        assert_eq!(observation.lifecycle, MatchLifecycle::InMatch);
        assert_eq!(observation.turn, Some(1));
        assert_eq!(observation.local_player_entity_id, Some(2));
        assert_eq!(observation.enemy_player_entity_id, Some(7));
        assert_eq!(observation.client_cards_drawn_count, 1);
        assert_eq!(observation.players.len(), 2);

        let player = observation
            .players
            .iter()
            .find(|player| player.participant == Participant::Player)
            .expect("local player observed");
        let player_hand = player
            .zones
            .iter()
            .find(|zone| zone.label == "Hand")
            .expect("hand zone");
        assert_eq!(player_hand.card_entity_ids, vec![21]);
        assert_eq!(player_hand.known_count, 1);

        let opponent = observation
            .players
            .iter()
            .find(|player| player.participant == Participant::Opponent)
            .expect("opponent observed");
        let opponent_deck = opponent
            .zones
            .iter()
            .find(|zone| zone.label == "Deck")
            .expect("opponent deck");
        assert_eq!(opponent_deck.hidden_count, 1);

        let abomination = observation
            .cards
            .iter()
            .find(|card| card.entity_id == 21)
            .expect("known card observed");
        assert_eq!(
            abomination.card_definition_key,
            Some(CardKey("Abomination".to_string()))
        );
        assert_eq!(abomination.owner, Participant::Player);
        assert_eq!(abomination.raw_zone.as_deref(), Some("Hand"));
        assert_eq!(abomination.previous_raw_zone.as_deref(), Some("Deck"));
        assert_eq!(abomination.domain_zone, Zone::Hand);
    }

    #[test]
    fn diffs_sanitized_transition_without_guessing_graveyard_semantics() {
        let before: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-active-turn.json"
        ))
        .expect("before fixture parses");
        let after: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-transition-after.json"
        ))
        .expect("after fixture parses");

        let before = observe_game_state(&before).expect("observes before");
        let after = observe_game_state(&after).expect("observes after");
        let transitions = diff_observations(&before, &after);

        assert_eq!(after.lifecycle, MatchLifecycle::InMatch);
        assert!(transitions.contains(&ObservedTransition::CardAppeared {
            entity_id: 22,
            owner: Participant::Player,
            card_definition_key: Some(CardKey("Viv".to_string())),
            raw_zone: Some("Hand".to_string()),
        }));
        assert!(
            transitions.contains(&ObservedTransition::CardDefinitionRevealed {
                entity_id: 31,
                card_definition_key: CardKey("Daken".to_string()),
            })
        );
        assert!(transitions.contains(&ObservedTransition::CardZoneChanged {
            entity_id: 31,
            from_raw_zone: Some("Hand".to_string()),
            to_raw_zone: Some("Location".to_string()),
            from_domain_zone: Zone::Hand,
            to_domain_zone: Zone::Board,
        }));
    }

    #[test]
    fn reconciles_initial_snapshot_into_match_started_and_observed_instances() {
        let value: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-active-turn.json"
        ))
        .expect("fixture parses");
        let observation = observe_game_state(&value).expect("observes game state");

        let events = reconcile_observation(ReconciliationInput {
            previous: None,
            current: &observation,
            snapshot_version: Some("fixture-1".to_string()),
        });

        assert!(events.contains(&MatchEvent::MatchStarted {
            snapshot_version: Some("fixture-1".to_string())
        }));
        assert!(events.iter().any(|event| matches!(
            event,
            MatchEvent::CardInstanceObserved { card }
                if card.internal_instance_id == CardInstanceId("game:21".to_string())
                    && card.card_definition_key == Some(CardKey("Abomination".to_string()))
                    && card.owner == Participant::Player
                    && card.current_zone == Zone::Hand
        )));
    }

    #[test]
    fn reconciles_draw_play_reveal_and_generated_observations() {
        let before: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-active-turn.json"
        ))
        .expect("before fixture parses");
        let after: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-transition-after.json"
        ))
        .expect("after fixture parses");
        let before = observe_game_state(&before).expect("observes before");
        let after = observe_game_state(&after).expect("observes after");

        let events = reconcile_observation(ReconciliationInput {
            previous: Some(&before),
            current: &after,
            snapshot_version: Some("fixture-2".to_string()),
        });

        assert!(events.contains(&MatchEvent::CardGenerated {
            card: CardInstanceId("game:22".to_string()),
            origin: CardOrigin::UnknownExternal,
        }));
        assert!(events.contains(&MatchEvent::CardRevealed {
            card: CardInstanceId("game:31".to_string()),
        }));
        assert!(events.contains(&MatchEvent::CardPlayed {
            card: CardInstanceId("game:31".to_string()),
        }));
    }

    #[test]
    fn reconciles_deck_to_hand_as_drawn() {
        let before: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-active-turn.json"
        ))
        .expect("before fixture parses");
        let after: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-draw-after.json"
        ))
        .expect("after fixture parses");
        let before = observe_game_state(&before).expect("observes before");
        let after = observe_game_state(&after).expect("observes after");

        let events = reconcile_observation(ReconciliationInput {
            previous: Some(&before),
            current: &after,
            snapshot_version: Some("fixture-draw".to_string()),
        });

        assert!(events.contains(&MatchEvent::CardDrawn {
            card: CardInstanceId("game:20".to_string()),
        }));
        assert!(events.contains(&MatchEvent::CardRevealed {
            card: CardInstanceId("game:20".to_string()),
        }));
    }

    #[test]
    fn reconciles_hand_to_graveyard_as_discarded() {
        let before: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-active-turn.json"
        ))
        .expect("before fixture parses");
        let after: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-discard-after.json"
        ))
        .expect("after fixture parses");
        let before = observe_game_state(&before).expect("observes before");
        let after = observe_game_state(&after).expect("observes after");

        let events = reconcile_observation(ReconciliationInput {
            previous: Some(&before),
            current: &after,
            snapshot_version: Some("fixture-discard".to_string()),
        });

        assert!(events.contains(&MatchEvent::CardDiscarded {
            card: CardInstanceId("game:21".to_string()),
        }));
        assert!(!events.contains(&MatchEvent::CardDestroyed {
            card: CardInstanceId("game:21".to_string()),
        }));
        assert!(!events.iter().any(|event| matches!(
            event,
            MatchEvent::UnknownTransitionObserved {
                card: Some(CardInstanceId(id)),
                ..
            } if id == "game:21"
        )));
    }

    #[test]
    fn reconciles_board_to_graveyard_as_destroyed() {
        let before: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-transition-after.json"
        ))
        .expect("before fixture parses");
        let after: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-graveyard-after.json"
        ))
        .expect("after fixture parses");
        let before = observe_game_state(&before).expect("observes before");
        let after = observe_game_state(&after).expect("observes after");

        let events = reconcile_observation(ReconciliationInput {
            previous: Some(&before),
            current: &after,
            snapshot_version: Some("fixture-graveyard".to_string()),
        });

        assert!(events.contains(&MatchEvent::CardDestroyed {
            card: CardInstanceId("game:31".to_string()),
        }));
        assert!(!events.contains(&MatchEvent::CardDiscarded {
            card: CardInstanceId("game:31".to_string()),
        }));
        assert!(!events.iter().any(|event| matches!(
            event,
            MatchEvent::UnknownTransitionObserved {
                card: Some(CardInstanceId(id)),
                ..
            } if id == "game:31"
        )));
    }

    #[test]
    fn projects_overlay_counts_and_consumed_original_cards() {
        let value: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-transition-after.json"
        ))
        .expect("fixture parses");
        let observation = observe_game_state(&value).expect("observes game state");

        let projection = project_overlay(&observation);

        assert_eq!(projection.lifecycle, MatchLifecycle::InMatch);
        assert_eq!(projection.player.deck_count, 1);
        assert_eq!(projection.player.hand_count, 2);
        assert_eq!(projection.opponent.deck_count, 1);
        assert_eq!(projection.opponent.hand_count, 0);
        assert_eq!(projection.opponent.board_count, 1);
        assert!(projection.opponent.cards.iter().any(|card| {
            card.instance_id == CardInstanceId("game:31".to_string())
                && card.card_definition_key == Some(CardKey("Daken".to_string()))
                && card.zone == Zone::Board
                && card.consumed_from_deck
        }));
    }

    #[test]
    fn stateful_projection_buckets_destroyed_and_discarded_cards() {
        let active: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-active-turn.json"
        ))
        .expect("active fixture parses");
        let discarded: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-discard-after.json"
        ))
        .expect("discard fixture parses");
        let board: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-transition-after.json"
        ))
        .expect("board fixture parses");
        let destroyed: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/snapshots/sanitized-graveyard-after.json"
        ))
        .expect("destroy fixture parses");

        let active = observe_game_state(&active).expect("observes active");
        let discarded = observe_game_state(&discarded).expect("observes discarded");
        let board = observe_game_state(&board).expect("observes board");
        let destroyed = observe_game_state(&destroyed).expect("observes destroyed");

        let mut projector = OverlayProjector::new();
        let initial_events = reconcile_observation(ReconciliationInput {
            previous: None,
            current: &active,
            snapshot_version: None,
        });
        let _ = projector.project(&active, &initial_events);
        let discard_events = reconcile_observation(ReconciliationInput {
            previous: Some(&active),
            current: &discarded,
            snapshot_version: None,
        });
        let discard_projection = projector.project(&discarded, &discard_events);

        assert_eq!(discard_projection.player.discarded_count, 1);
        assert_eq!(discard_projection.player.destroyed_count, 0);
        assert!(discard_projection.player.cards.iter().any(|card| {
            card.instance_id == CardInstanceId("game:21".to_string())
                && card.zone == Zone::Discarded
        }));

        let mut projector = OverlayProjector::new();
        let board_events = reconcile_observation(ReconciliationInput {
            previous: None,
            current: &board,
            snapshot_version: None,
        });
        let _ = projector.project(&board, &board_events);
        let destroy_events = reconcile_observation(ReconciliationInput {
            previous: Some(&board),
            current: &destroyed,
            snapshot_version: None,
        });
        let destroy_projection = projector.project(&destroyed, &destroy_events);

        assert_eq!(destroy_projection.opponent.destroyed_count, 1);
        assert_eq!(destroy_projection.opponent.discarded_count, 0);
        assert!(destroy_projection.opponent.cards.iter().any(|card| {
            card.instance_id == CardInstanceId("game:31".to_string())
                && card.zone == Zone::Destroyed
        }));
    }
}
