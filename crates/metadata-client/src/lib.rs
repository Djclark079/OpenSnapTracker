//! Versioned metadata contracts used between the client and project service.

use domain::{CardDefinition, CardKey, CollectableState, ReleaseState};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CatalogueDeltaRequest {
    pub after_revision: u64,
    pub client_schema_version: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CatalogueDeltaResponse {
    pub schema_version: u16,
    pub from_revision: u64,
    pub to_revision: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub generated_at: OffsetDateTime,
    pub cards: Vec<NormalizedCardRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NormalizedCardRecord {
    pub key: String,
    pub name: String,
    pub cost: i16,
    pub power: i16,
    pub ability: String,
    pub collectable: bool,
    pub released: bool,
    pub series: Option<String>,
    pub image_url: Option<String>,
    pub revision: u64,
}

impl From<NormalizedCardRecord> for CardDefinition {
    fn from(record: NormalizedCardRecord) -> Self {
        Self {
            key: CardKey(record.key),
            name: record.name,
            base_cost: record.cost,
            base_power: record.power,
            canonical_ability_text: record.ability,
            collectable: if record.collectable {
                CollectableState::Collectable
            } else {
                CollectableState::NotCollectable
            },
            release_state: if record.released {
                ReleaseState::Released
            } else {
                ReleaseState::Unreleased
            },
            series: record.series,
            image_url: record.image_url,
            metadata_revision: record.revision,
        }
    }
}

#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("metadata delta moved backwards from {from_revision} to {to_revision}")]
    RevisionMovedBackwards {
        from_revision: u64,
        to_revision: u64,
    },
    #[error("metadata response schema {actual} is unsupported; expected {expected}")]
    UnsupportedSchema { expected: u16, actual: u16 },
}

pub fn validate_delta(
    response: &CatalogueDeltaResponse,
    expected_schema: u16,
) -> Result<(), MetadataError> {
    if response.schema_version != expected_schema {
        return Err(MetadataError::UnsupportedSchema {
            expected: expected_schema,
            actual: response.schema_version,
        });
    }
    if response.to_revision < response.from_revision {
        return Err(MetadataError::RevisionMovedBackwards {
            from_revision: response.from_revision,
            to_revision: response.to_revision,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_normalized_record_to_domain_definition() {
        let record = NormalizedCardRecord {
            key: "Abomination".to_string(),
            name: "Abomination".to_string(),
            cost: 5,
            power: 9,
            ability: "Foolish rabble! You are beneath me!".to_string(),
            collectable: true,
            released: true,
            series: Some("Starter".to_string()),
            image_url: Some("https://example.test/abomination.webp".to_string()),
            revision: 7,
        };
        let definition: CardDefinition = record.into();
        assert_eq!(definition.key.0, "Abomination");
        assert_eq!(definition.metadata_revision, 7);
        assert_eq!(definition.collectable, CollectableState::Collectable);
    }

    #[test]
    fn rejects_backwards_revision() {
        let response = CatalogueDeltaResponse {
            schema_version: 1,
            from_revision: 10,
            to_revision: 9,
            generated_at: OffsetDateTime::UNIX_EPOCH,
            cards: Vec::new(),
        };
        assert!(matches!(
            validate_delta(&response, 1),
            Err(MetadataError::RevisionMovedBackwards { .. })
        ));
    }
}
