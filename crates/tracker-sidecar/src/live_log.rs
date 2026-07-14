use domain::{CardInstanceId, CardKey, CardKnowledge, Zone};
use state_reader::{TextOverlayCard, TextOverlayPayload, TextOverlaySlotState};
use std::collections::{HashMap, HashSet};

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
    LocationRevealed {
        key: String,
    },
    MatchLeft,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LiveLogState {
    player_deck_keys: HashSet<CardKey>,
    player_consumed_keys: HashSet<CardKey>,
    player_away_keys: Vec<CardKey>,
    player_hand_keys: Vec<CardKey>,
    player_supplemental: Vec<CardKey>,
    player_destroyed: Vec<CardKey>,
    opponent_known: Vec<CardKey>,
    opponent_consumed_keys: HashSet<CardKey>,
    staged_by_entity: HashMap<String, CardKey>,
    recently_player_resolved: HashSet<CardKey>,
    pending_hand_swap: bool,
    pending_hand_swap_hand_events: u8,
}

impl LiveLogState {
    pub fn set_player_deck<I>(&mut self, keys: I)
    where
        I: IntoIterator<Item = CardKey>,
    {
        self.player_deck_keys = keys.into_iter().collect();
    }

    pub fn apply(&mut self, event: LiveLogEvent) -> bool {
        match event {
            LiveLogEvent::PlayerDrew { key } => {
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
                if let Some(entity_id) = entity_id {
                    self.staged_by_entity.insert(entity_id, key.clone());
                }
                self.record_player_card_seen(key, PlayerCardSeenSource::Staged);
                true
            }
            LiveLogEvent::PlayerStageAccepted { entity_id, undo } => {
                if undo {
                    if let Some(entity_id) = entity_id {
                        self.staged_by_entity.remove(&entity_id);
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
            LiveLogEvent::LocationRevealed { key } => {
                if key == "ThePeak" {
                    self.pending_hand_swap = true;
                    self.pending_hand_swap_hand_events = 0;
                }
                true
            }
            LiveLogEvent::MatchLeft => {
                self.clear_match_state();
                true
            }
        }
    }

    pub fn apply_to_payload(&self, payload: &mut TextOverlayPayload) {
        for slot in &mut payload.player.deck_slots {
            if let Some(card) = &mut slot.card
                && card
                    .card_definition_key
                    .as_ref()
                    .is_some_and(|key| self.player_consumed_keys.contains(key))
            {
                card.consumed_from_deck = true;
                if card
                    .card_definition_key
                    .as_ref()
                    .is_some_and(|key| self.player_away_keys.contains(key))
                {
                    card.zone = Zone::UnknownTransition;
                    card.raw_zone = Some("Player.log:away".to_string());
                }
            }
        }

        for key in &self.player_supplemental {
            if !payload
                .player
                .supplemental
                .iter()
                .any(|card| card.card_definition_key.as_ref() == Some(key))
            {
                payload.player.supplemental.push(live_text_card(
                    "player-log:supplemental",
                    payload.player.supplemental.len(),
                    key,
                    Zone::Hand,
                    false,
                ));
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
                ));
            }
        }
        payload.player.counters.destroyed = payload.player.destroyed.len();

        for key in &self.opponent_known {
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
                ));
            }
        }
    }

    fn record_player_card_seen(&mut self, key: CardKey, source: PlayerCardSeenSource) {
        if self.pending_hand_swap && source == PlayerCardSeenSource::Hand {
            self.pending_hand_swap_hand_events =
                self.pending_hand_swap_hand_events.saturating_add(1);
            if !self.player_deck_keys.contains(&key) {
                if let Some(away_key) = self.player_hand_keys.first().cloned() {
                    push_unique(&mut self.player_away_keys, away_key.clone());
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
            push_unique(&mut self.player_supplemental, key.clone());
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
        self.opponent_known.clear();
        self.opponent_consumed_keys.clear();
        self.staged_by_entity.clear();
        self.recently_player_resolved.clear();
        self.pending_hand_swap = false;
        self.pending_hand_swap_hand_events = 0;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlayerCardSeenSource {
    Hand,
    Staged,
}

pub fn parse_live_log_line(line: &str) -> Option<LiveLogEvent> {
    if line == "GameInputHandler|OnLeaveGame" {
        return Some(LiveLogEvent::MatchLeft);
    }
    if line.contains("|DrawCard") {
        return parse_card_vfx_key(line).map(|key| LiveLogEvent::PlayerDrew {
            key: CardKey(key.to_string()),
        });
    }
    if line.contains("|HighlightCardPlayable") || line.contains("|HighlightCardUnplayable") {
        return parse_card_vfx_key(line).map(|key| LiveLogEvent::PlayerHandObserved {
            key: CardKey(key.to_string()),
        });
    }
    if line.contains("|RevealLocation") {
        return parse_location_vfx_key(line).map(|key| LiveLogEvent::LocationRevealed {
            key: key.to_string(),
        });
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
    if line.contains("|ThisCardDestroyedTrigger") {
        return parse_card_vfx_key(line).map(|key| LiveLogEvent::CardDestroyedTrigger {
            key: CardKey(key.to_string()),
        });
    }
    None
}

fn live_text_card(
    prefix: &str,
    index: usize,
    key: &CardKey,
    zone: Zone,
    consumed_from_deck: bool,
) -> TextOverlayCard {
    TextOverlayCard {
        instance_id: CardInstanceId(format!("{prefix}:{index}:{}", key.0)),
        label: key.0.clone(),
        card_definition_key: Some(key.clone()),
        knowledge: CardKnowledge::KnownCard,
        zone,
        raw_zone: Some("Player.log".to_string()),
        original_deck_candidate: consumed_from_deck,
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
        assert_eq!(state.player_supplemental, vec![CardKey("Rock".to_string())]);
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
            vec![CardKey("Sentinel".to_string())]
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
            vec![CardKey("BlackPanther".to_string())]
        );
        assert!(
            state
                .player_consumed_keys
                .contains(&CardKey("Nova".to_string()))
        );
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
}
