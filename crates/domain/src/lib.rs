//! Core domain types for OpenSnapTracker.
//!
//! These types intentionally model what the tracker needs without claiming that
//! Marvel Snap state-file semantics are fully known.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct CardKey(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CardDefinition {
    pub key: CardKey,
    pub name: String,
    pub base_cost: i16,
    pub base_power: i16,
    pub canonical_ability_text: String,
    pub collectable: CollectableState,
    pub release_state: ReleaseState,
    pub series: Option<String>,
    pub image_url: Option<String>,
    pub metadata_revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectableState {
    Collectable,
    NotCollectable,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseState {
    Released,
    Unreleased,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct CardInstanceId(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CardInstance {
    pub internal_instance_id: CardInstanceId,
    pub external_game_instance_id: Option<String>,
    pub card_definition_key: Option<CardKey>,
    pub owner: Participant,
    pub controller: Participant,
    pub origin: CardOrigin,
    pub current_zone: Zone,
    pub previous_zone: Option<Zone>,
    pub original_deck_slot: Option<OriginalDeckSlot>,
    pub knowledge: CardKnowledge,
    pub provenance: Provenance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Participant {
    Player,
    Opponent,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardOrigin {
    OriginalDeck,
    Generated,
    Copied,
    Transferred,
    Stolen,
    AddedToHand,
    AddedToDeck,
    TransformedResult,
    UnknownExternal,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Zone {
    Deck,
    Hand,
    Board,
    Destroyed,
    Discarded,
    RemovedConfirmed,
    Transformed,
    Merged,
    Returned,
    UnknownTransition,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OriginalDeckSlot(pub u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardKnowledge {
    UnknownCard,
    KnownCard,
    Inferred,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    pub confidence: Confidence,
    pub source: ProvenanceSource,
    pub notes: Vec<String>,
}

impl Provenance {
    #[must_use]
    pub fn observed() -> Self {
        Self {
            confidence: Confidence::Observed,
            source: ProvenanceSource::Snapshot,
            notes: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Observed,
    InferredHigh,
    InferredLow,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceSource {
    Snapshot,
    Reconciliation,
    Metadata,
    UserFixture,
    Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerState {
    pub original_deck_slots: Vec<Option<CardInstance>>,
    pub supplemental_cards: Vec<CardInstance>,
    pub deck_count: u16,
    pub hand_count: u16,
    pub destroyed_cards: Vec<CardInstance>,
    pub discarded_cards: Vec<CardInstance>,
    pub removed_cards: Vec<CardInstance>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchState {
    pub player: PlayerState,
    pub opponent: PlayerState,
    pub lifecycle: MatchLifecycle,
    pub current_snapshot_version: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_update_timestamp: Option<OffsetDateTime>,
    pub diagnostics: Diagnostics,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchLifecycle {
    #[default]
    Unknown,
    MainMenu,
    Matchmaking,
    MatchStarted,
    InMatch,
    MatchEnded,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Diagnostics {
    pub snapshot_hash: Option<String>,
    pub parse_warnings: Vec<String>,
    pub unknown_fields: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MatchEvent {
    MatchStarted {
        snapshot_version: Option<String>,
    },
    MatchEnded,
    DeckIdentified {
        participant: Participant,
        cards: Vec<CardInstanceId>,
    },
    CardInstanceObserved {
        card: CardInstance,
    },
    CardDrawn {
        card: CardInstanceId,
    },
    CardPlayed {
        card: CardInstanceId,
    },
    CardRevealed {
        card: CardInstanceId,
    },
    CardReturned {
        card: CardInstanceId,
        to_zone: Zone,
    },
    CardDestroyed {
        card: CardInstanceId,
    },
    CardDiscarded {
        card: CardInstanceId,
    },
    CardRemoved {
        card: CardInstanceId,
        reason: RemovalReason,
    },
    CardGenerated {
        card: CardInstanceId,
        origin: CardOrigin,
    },
    CardTransferred {
        card: CardInstanceId,
        from: Participant,
        to: Participant,
    },
    CardTransformed {
        from: CardInstanceId,
        to: CardInstanceId,
    },
    CardMerged {
        from: CardInstanceId,
        into: CardInstanceId,
    },
    SnapshotParseWarning {
        message: String,
    },
    UnknownTransitionObserved {
        card: Option<CardInstanceId>,
        details: Value,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovalReason {
    Destroyed,
    Discarded,
    RemovedConfirmed,
    Transformed,
    Merged,
    Returned,
    UnknownTransition,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supplemental_origins_are_distinct_from_removed_zones() {
        assert_ne!(CardOrigin::Generated, CardOrigin::OriginalDeck);
        assert_ne!(Zone::Destroyed, Zone::RemovedConfirmed);
        assert_ne!(Zone::UnknownTransition, Zone::RemovedConfirmed);
    }

    #[test]
    fn serializes_event_type_names() {
        let event = MatchEvent::CardDestroyed {
            card: CardInstanceId("card-1".to_string()),
        };
        let json = serde_json::to_value(event).expect("event serializes");
        assert_eq!(json["type"], "card_destroyed");
    }
}
