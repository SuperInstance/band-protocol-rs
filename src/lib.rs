//! Wire protocol for inter-agent communication in a musical ensemble.
//!
//! This crate defines the types and serialization format used to pass
//! musical messages between agents in a band simulation.

/// Timing primitives rooted in T-minus musical time.
pub mod timestamp {
    /// A musical timestamp expressed as beat count, intra-beat phase, and
    /// current tempo.  Phase is normalised to `[0.0, 1.0)`.
    #[derive(Debug, Clone, PartialEq)]
    pub struct TMinusTimestamp {
        /// The current beat index (0-based).
        pub beat: u64,
        /// Fractional position within the current beat; always in `[0.0, 1.0)`.
        pub phase: f64,
        /// Tempo in beats per minute at the moment the timestamp was recorded.
        pub tempo_bpm: f64,
    }

    impl TMinusTimestamp {
        /// Create a new timestamp at the given beat and phase with the supplied
        /// tempo.
        ///
        /// # Panics
        /// Panics if `tempo_bpm` is not positive.
        #[must_use]
        pub fn new(beat: u64, phase: f64, tempo_bpm: f64) -> Self {
            assert!(tempo_bpm > 0.0, "tempo_bpm must be positive");
            let phase = phase.rem_euclid(1.0);
            Self { beat, phase, tempo_bpm }
        }

        /// Advance the timestamp by `dt_seconds` of wall-clock time.
        ///
        /// The phase accumulates; when it overflows 1.0 the beat counter is
        /// incremented accordingly.
        pub fn advance(&mut self, dt_seconds: f64) {
            let beats_elapsed = dt_seconds * self.tempo_bpm / 60.0;
            let total_phase = self.phase + beats_elapsed;
            let extra_beats = total_phase.floor() as u64;
            self.beat = self.beat.saturating_add(extra_beats);
            self.phase = total_phase.rem_euclid(1.0);
        }
    }

    impl PartialOrd for TMinusTimestamp {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            match self.beat.cmp(&other.beat) {
                std::cmp::Ordering::Equal => self.phase.partial_cmp(&other.phase),
                ord => Some(ord),
            }
        }
    }
}

/// Conservation metadata attached to every message.
pub mod header {
    /// Energy and dial-position metadata used to enforce conservation laws
    /// across the ensemble.
    #[derive(Debug, Clone, PartialEq)]
    pub struct ConservationMeta {
        /// Non-negative energy value associated with the message.
        pub energy: f64,
        /// Three-dimensional dial position; each component is in `[0.0, 1.0]`.
        pub dial_position: [f64; 3],
    }

    impl ConservationMeta {
        /// Construct a new `ConservationMeta`.
        #[must_use]
        pub fn new(energy: f64, dial_position: [f64; 3]) -> Self {
            Self { energy, dial_position }
        }

        /// Returns `true` if `energy >= 0.0` and every dial component is in
        /// `[0.0, 1.0]`.
        #[must_use]
        pub fn is_valid(&self) -> bool {
            self.energy >= 0.0
                && self.dial_position.iter().all(|&d| (0.0..=1.0).contains(&d))
        }
    }
}

/// Typed message payloads exchanged between band agents.
pub mod message {
    use crate::header::ConservationMeta;
    use crate::timestamp::TMinusTimestamp;

    /// The content carried by a [`BandMessage`].
    #[derive(Debug, Clone, PartialEq)]
    pub enum MessagePayload {
        /// A metronome-style beat event with a subdivision hint.
        Beat {
            /// Number of subdivisions within the beat (e.g. 4 for sixteenth
            /// notes at quarter-note tempo).
            subdivision: u8,
        },
        /// A chord-change directive.
        ChordChange {
            /// MIDI note number of the chord root.
            root: u8,
            /// Chord quality identifier (0 = major, 1 = minor, …).
            quality: u8,
        },
        /// A tempo modulation request.
        TempoShift {
            /// The new tempo in beats per minute.
            new_bpm: f64,
        },
        /// A rest directive specifying how long an agent should remain silent.
        Tacet {
            /// Duration of the rest, measured in beats.
            duration_beats: f64,
        },
        /// An entry cue for a specific instrumental role.
        Entry {
            /// Role identifier matching [`crate::channel::InstrumentChannel`]
            /// discriminants.
            role: u8,
        },
    }

    /// A complete message routed between band agents.
    #[derive(Debug, Clone, PartialEq)]
    pub struct BandMessage {
        /// Unique identifier of the originating agent.
        pub agent_id: u64,
        /// Musical timestamp at the moment of emission.
        pub timestamp: TMinusTimestamp,
        /// The content of this message.
        pub payload: MessagePayload,
        /// Conservation metadata.
        pub conservation: ConservationMeta,
    }

    impl BandMessage {
        /// Construct a new [`BandMessage`].
        #[must_use]
        pub fn new(
            agent_id: u64,
            timestamp: TMinusTimestamp,
            payload: MessagePayload,
            conservation: ConservationMeta,
        ) -> Self {
            Self { agent_id, timestamp, payload, conservation }
        }
    }
}

/// Instrument channel filtering.
pub mod channel {
    /// The instrument channels recognised by the protocol.
    #[derive(Debug, Clone, PartialEq)]
    pub enum InstrumentChannel {
        /// Percussion / drum kit (role 0).
        Drums,
        /// Bass instruments (role 1).
        Bass,
        /// Keyboard instruments (role 2).
        Keys,
        /// Brass and woodwind horns (role 3).
        Horns,
        /// Sustained pad sounds (role 4).
        Pads,
        /// Broadcast channel; matches every role.
        All,
    }

    impl InstrumentChannel {
        /// Returns the canonical role byte for this channel, or `None` for
        /// [`InstrumentChannel::All`].
        #[must_use]
        pub fn role_byte(&self) -> Option<u8> {
            match self {
                Self::Drums => Some(0),
                Self::Bass => Some(1),
                Self::Keys => Some(2),
                Self::Horns => Some(3),
                Self::Pads => Some(4),
                Self::All => None,
            }
        }
    }

    /// A filter that accepts messages addressed to a specific instrument
    /// channel.
    #[derive(Debug, Clone)]
    pub struct ChannelFilter {
        /// The channel this filter accepts.
        pub channel: InstrumentChannel,
    }

    impl ChannelFilter {
        /// Create a new filter for `channel`.
        #[must_use]
        pub fn new(channel: InstrumentChannel) -> Self {
            Self { channel }
        }

        /// Returns `true` if `role` matches this filter's channel.
        ///
        /// [`InstrumentChannel::All`] always matches.
        #[must_use]
        pub fn matches(&self, role: u8) -> bool {
            match self.channel.role_byte() {
                Some(r) => r == role,
                None => true,
            }
        }
    }
}

/// Binary serialization and deserialization for [`message::BandMessage`].
///
/// # Wire Format
///
/// ```text
/// [agent_id : 8 bytes le u64]
/// [beat     : 8 bytes le u64]
/// [phase    : 8 bytes le f64]
/// [tempo    : 8 bytes le f64]
/// [energy   : 8 bytes le f64]
/// [dial0    : 8 bytes le f64]
/// [dial1    : 8 bytes le f64]
/// [dial2    : 8 bytes le f64]
/// [tag      : 1 byte        ]
/// [payload  : 16 bytes      ]
/// ──────────────────────────
/// Total      81 bytes
/// ```
///
/// Payload tags: `0` = Beat, `1` = ChordChange, `2` = TempoShift,
/// `3` = Tacet, `4` = Entry.
pub mod serialize {
    use crate::header::ConservationMeta;
    use crate::message::{BandMessage, MessagePayload};
    use crate::timestamp::TMinusTimestamp;

    /// Total wire size of an encoded [`BandMessage`].
    ///
    /// 8 fields × 8 bytes + 1 tag byte + 16 payload bytes = 81 bytes.
    pub const ENCODED_LEN: usize = 81;

    /// Encode `msg` into a 73-byte `Vec<u8>`.
    #[must_use]
    pub fn encode(msg: &BandMessage) -> Vec<u8> {
        let mut buf = Vec::with_capacity(ENCODED_LEN);

        buf.extend_from_slice(&msg.agent_id.to_le_bytes());
        buf.extend_from_slice(&msg.timestamp.beat.to_le_bytes());
        buf.extend_from_slice(&msg.timestamp.phase.to_le_bytes());
        buf.extend_from_slice(&msg.timestamp.tempo_bpm.to_le_bytes());
        buf.extend_from_slice(&msg.conservation.energy.to_le_bytes());
        buf.extend_from_slice(&msg.conservation.dial_position[0].to_le_bytes());
        buf.extend_from_slice(&msg.conservation.dial_position[1].to_le_bytes());
        buf.extend_from_slice(&msg.conservation.dial_position[2].to_le_bytes());

        let (tag, payload_bytes) = encode_payload(&msg.payload);
        buf.push(tag);
        buf.extend_from_slice(&payload_bytes);

        debug_assert_eq!(buf.len(), ENCODED_LEN);
        buf
    }

    /// Decode a [`BandMessage`] from the supplied byte slice.
    ///
    /// # Errors
    /// Returns `Err` if `bytes` is shorter than [`ENCODED_LEN`] or the
    /// payload tag is unknown.
    pub fn decode(bytes: &[u8]) -> Result<BandMessage, &'static str> {
        if bytes.len() < ENCODED_LEN {
            return Err("buffer too short");
        }

        let agent_id = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        let beat = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
        let phase = f64::from_le_bytes(bytes[16..24].try_into().unwrap());
        let tempo_bpm = f64::from_le_bytes(bytes[24..32].try_into().unwrap());
        let energy = f64::from_le_bytes(bytes[32..40].try_into().unwrap());
        let dial0 = f64::from_le_bytes(bytes[40..48].try_into().unwrap());
        let dial1 = f64::from_le_bytes(bytes[48..56].try_into().unwrap());
        let dial2 = f64::from_le_bytes(bytes[56..64].try_into().unwrap());
        let tag = bytes[64];
        let payload_bytes: [u8; 16] = bytes[65..81].try_into().unwrap();

        let payload = decode_payload(tag, &payload_bytes)?;

        let timestamp = TMinusTimestamp { beat, phase, tempo_bpm };
        let conservation = ConservationMeta::new(energy, [dial0, dial1, dial2]);

        Ok(BandMessage::new(agent_id, timestamp, payload, conservation))
    }

    fn encode_payload(payload: &MessagePayload) -> (u8, [u8; 16]) {
        let mut data = [0u8; 16];
        match payload {
            MessagePayload::Beat { subdivision } => {
                data[0] = *subdivision;
                (0, data)
            }
            MessagePayload::ChordChange { root, quality } => {
                data[0] = *root;
                data[1] = *quality;
                (1, data)
            }
            MessagePayload::TempoShift { new_bpm } => {
                data[0..8].copy_from_slice(&new_bpm.to_le_bytes());
                (2, data)
            }
            MessagePayload::Tacet { duration_beats } => {
                data[0..8].copy_from_slice(&duration_beats.to_le_bytes());
                (3, data)
            }
            MessagePayload::Entry { role } => {
                data[0] = *role;
                (4, data)
            }
        }
    }

    fn decode_payload(tag: u8, data: &[u8; 16]) -> Result<MessagePayload, &'static str> {
        match tag {
            0 => Ok(MessagePayload::Beat { subdivision: data[0] }),
            1 => Ok(MessagePayload::ChordChange { root: data[0], quality: data[1] }),
            2 => {
                let new_bpm = f64::from_le_bytes(data[0..8].try_into().unwrap());
                Ok(MessagePayload::TempoShift { new_bpm })
            }
            3 => {
                let duration_beats = f64::from_le_bytes(data[0..8].try_into().unwrap());
                Ok(MessagePayload::Tacet { duration_beats })
            }
            4 => Ok(MessagePayload::Entry { role: data[0] }),
            _ => Err("unknown payload tag"),
        }
    }

    /// Convenience: encode then immediately decode, useful for round-trip
    /// testing.
    pub fn round_trip(msg: &BandMessage) -> Result<BandMessage, &'static str> {
        decode(&encode(msg))
    }

}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::channel::{ChannelFilter, InstrumentChannel};
    use crate::header::ConservationMeta;
    use crate::message::{BandMessage, MessagePayload};
    use crate::serialize;
    use crate::timestamp::TMinusTimestamp;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn ts(beat: u64, phase: f64) -> TMinusTimestamp {
        TMinusTimestamp::new(beat, phase, 120.0)
    }

    fn meta() -> ConservationMeta {
        ConservationMeta::new(1.0, [0.5, 0.5, 0.5])
    }

    fn beat_msg(agent_id: u64) -> BandMessage {
        BandMessage::new(agent_id, ts(0, 0.0), MessagePayload::Beat { subdivision: 4 }, meta())
    }

    // ── TMinusTimestamp::new ─────────────────────────────────────────────────

    #[test]
    fn timestamp_new_stores_fields() {
        let t = TMinusTimestamp::new(3, 0.25, 120.0);
        assert_eq!(t.beat, 3);
        assert!((t.phase - 0.25).abs() < 1e-12);
        assert!((t.tempo_bpm - 120.0).abs() < 1e-12);
    }

    #[test]
    fn timestamp_new_normalises_phase() {
        // phase 1.75 -> beat + 1, phase 0.75
        let t = TMinusTimestamp::new(0, 1.75, 120.0);
        assert!((t.phase - 0.75).abs() < 1e-12, "phase={}", t.phase);
    }

    // ── TMinusTimestamp::advance ─────────────────────────────────────────────

    #[test]
    fn advance_half_beat() {
        // At 120 BPM a beat is 0.5 s; advance by 0.25 s → phase = 0.5
        let mut t = TMinusTimestamp::new(0, 0.0, 120.0);
        t.advance(0.25);
        assert_eq!(t.beat, 0);
        assert!((t.phase - 0.5).abs() < 1e-9, "phase={}", t.phase);
    }

    #[test]
    fn advance_phase_wraps_at_one() {
        // Advance exactly one beat; phase should return to 0.0, beat increments
        let mut t = TMinusTimestamp::new(1, 0.0, 120.0);
        t.advance(0.5); // one beat at 120 BPM
        assert_eq!(t.beat, 2);
        assert!(t.phase < 1e-9, "phase={}", t.phase);
    }

    #[test]
    fn advance_multiple_beats() {
        let mut t = TMinusTimestamp::new(0, 0.0, 60.0); // 1 beat/s
        t.advance(3.5);
        assert_eq!(t.beat, 3);
        assert!((t.phase - 0.5).abs() < 1e-9, "phase={}", t.phase);
    }

    #[test]
    fn advance_beat_counter_increments() {
        let mut t = TMinusTimestamp::new(10, 0.9, 120.0);
        // 0.9 phase + advance of 0.1 beat (0.05 s) → phase = 1.0 → beat 11
        t.advance(0.05);
        assert_eq!(t.beat, 11);
    }

    // ── TMinusTimestamp ordering ──────────────────────────────────────────────

    #[test]
    fn ordering_earlier_beat_less() {
        let a = ts(1, 0.9);
        let b = ts(2, 0.0);
        assert!(a < b);
    }

    #[test]
    fn ordering_same_beat_phase_comparison() {
        let a = ts(5, 0.3);
        let b = ts(5, 0.7);
        assert!(a < b);
        assert!(b > a);
    }

    #[test]
    fn ordering_equal_timestamps() {
        let a = ts(3, 0.5);
        let b = ts(3, 0.5);
        assert!(!(a < b));
        assert!(!(a > b));
    }

    // ── ConservationMeta::is_valid ────────────────────────────────────────────

    #[test]
    fn conservation_zero_energy_is_valid() {
        let m = ConservationMeta::new(0.0, [0.5, 0.5, 0.5]);
        assert!(m.is_valid());
    }

    #[test]
    fn conservation_negative_energy_invalid() {
        let m = ConservationMeta::new(-0.001, [0.5, 0.5, 0.5]);
        assert!(!m.is_valid());
    }

    #[test]
    fn conservation_dial_at_boundary_valid() {
        let m = ConservationMeta::new(1.0, [0.0, 1.0, 0.5]);
        assert!(m.is_valid());
    }

    #[test]
    fn conservation_dial_out_of_range_invalid() {
        let m = ConservationMeta::new(1.0, [0.5, 1.001, 0.5]);
        assert!(!m.is_valid());
    }

    #[test]
    fn conservation_all_dials_zero_valid() {
        let m = ConservationMeta::new(0.0, [0.0, 0.0, 0.0]);
        assert!(m.is_valid());
    }

    // ── BandMessage construction ──────────────────────────────────────────────

    #[test]
    fn message_beat_construction() {
        let msg = beat_msg(1);
        assert_eq!(msg.agent_id, 1);
        assert_eq!(msg.payload, MessagePayload::Beat { subdivision: 4 });
    }

    #[test]
    fn message_chord_change_construction() {
        let msg = BandMessage::new(
            2,
            ts(0, 0.0),
            MessagePayload::ChordChange { root: 60, quality: 0 },
            meta(),
        );
        assert_eq!(msg.payload, MessagePayload::ChordChange { root: 60, quality: 0 });
    }

    #[test]
    fn message_tempo_shift_construction() {
        let msg = BandMessage::new(
            3,
            ts(0, 0.0),
            MessagePayload::TempoShift { new_bpm: 140.0 },
            meta(),
        );
        assert_eq!(msg.payload, MessagePayload::TempoShift { new_bpm: 140.0 });
    }

    #[test]
    fn message_tacet_construction() {
        let msg = BandMessage::new(
            4,
            ts(0, 0.0),
            MessagePayload::Tacet { duration_beats: 4.0 },
            meta(),
        );
        assert_eq!(msg.payload, MessagePayload::Tacet { duration_beats: 4.0 });
    }

    #[test]
    fn message_entry_construction() {
        let msg = BandMessage::new(
            5,
            ts(0, 0.0),
            MessagePayload::Entry { role: 2 },
            meta(),
        );
        assert_eq!(msg.payload, MessagePayload::Entry { role: 2 });
    }

    // ── serialize round-trips ─────────────────────────────────────────────────

    fn round_trip(msg: &BandMessage) -> BandMessage {
        serialize::round_trip(msg).expect("round-trip failed")
    }

    #[test]
    fn roundtrip_beat() {
        let msg = BandMessage::new(
            42,
            ts(7, 0.25),
            MessagePayload::Beat { subdivision: 8 },
            ConservationMeta::new(2.5, [0.1, 0.9, 0.5]),
        );
        assert_eq!(round_trip(&msg), msg);
    }

    #[test]
    fn roundtrip_chord_change() {
        let msg = BandMessage::new(
            1,
            ts(0, 0.0),
            MessagePayload::ChordChange { root: 69, quality: 1 },
            meta(),
        );
        assert_eq!(round_trip(&msg), msg);
    }

    #[test]
    fn roundtrip_tempo_shift() {
        let msg = BandMessage::new(
            10,
            ts(3, 0.75),
            MessagePayload::TempoShift { new_bpm: 180.0 },
            meta(),
        );
        assert_eq!(round_trip(&msg), msg);
    }

    #[test]
    fn roundtrip_tacet() {
        let msg = BandMessage::new(
            99,
            ts(1, 0.5),
            MessagePayload::Tacet { duration_beats: 8.0 },
            meta(),
        );
        assert_eq!(round_trip(&msg), msg);
    }

    #[test]
    fn roundtrip_entry() {
        let msg = BandMessage::new(
            7,
            ts(0, 0.0),
            MessagePayload::Entry { role: 3 },
            meta(),
        );
        assert_eq!(round_trip(&msg), msg);
    }

    #[test]
    fn roundtrip_preserves_conservation_meta() {
        let conservation = ConservationMeta::new(99.9, [0.0, 0.5, 1.0]);
        let msg = BandMessage::new(
            55,
            ts(100, 0.123),
            MessagePayload::Beat { subdivision: 1 },
            conservation.clone(),
        );
        let decoded = round_trip(&msg);
        assert!((decoded.conservation.energy - conservation.energy).abs() < 1e-10);
        for i in 0..3 {
            assert!(
                (decoded.conservation.dial_position[i]
                    - conservation.dial_position[i])
                    .abs()
                    < 1e-10
            );
        }
    }

    #[test]
    fn decode_error_on_short_buffer() {
        let result = serialize::decode(&[0u8; 10]);
        assert!(result.is_err());
    }

    #[test]
    fn decode_error_on_unknown_tag() {
        let msg = beat_msg(1);
        let mut bytes = serialize::encode(&msg);
        bytes[64] = 99; // corrupt the payload tag
        assert!(serialize::decode(&bytes).is_err());
    }

    // ── ChannelFilter::matches ────────────────────────────────────────────────

    #[test]
    fn filter_drums_matches_role_zero() {
        let f = ChannelFilter::new(InstrumentChannel::Drums);
        assert!(f.matches(0));
        assert!(!f.matches(1));
    }

    #[test]
    fn filter_bass_matches_role_one() {
        let f = ChannelFilter::new(InstrumentChannel::Bass);
        assert!(f.matches(1));
        assert!(!f.matches(0));
    }

    #[test]
    fn filter_keys_matches_role_two() {
        let f = ChannelFilter::new(InstrumentChannel::Keys);
        assert!(f.matches(2));
        assert!(!f.matches(3));
    }

    #[test]
    fn filter_horns_matches_role_three() {
        let f = ChannelFilter::new(InstrumentChannel::Horns);
        assert!(f.matches(3));
        assert!(!f.matches(4));
    }

    #[test]
    fn filter_pads_matches_role_four() {
        let f = ChannelFilter::new(InstrumentChannel::Pads);
        assert!(f.matches(4));
        assert!(!f.matches(0));
    }

    #[test]
    fn filter_all_matches_every_role() {
        let f = ChannelFilter::new(InstrumentChannel::All);
        for role in 0..=10u8 {
            assert!(f.matches(role), "All should match role {role}");
        }
    }
}
