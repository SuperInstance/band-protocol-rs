#![forbid(unsafe_code)]

pub const PROTOCOL_VERSION: u8 = 1;

mod tag {
    pub const MIDI: u8 = 0x01;
    pub const TMINUS_TICK: u8 = 0x02;
    pub const AGENT_SYNC: u8 = 0x03;
    pub const ENSEMBLE_STATE: u8 = 0x04;
    pub const HEARTBEAT: u8 = 0x05;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    InvalidVersion { expected: u8, got: u8 },
    UnknownMessageKind(u8),
    BufferTooShort { needed: usize, got: usize },
}

impl core::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidVersion { expected, got } => write!(f, "invalid version {got} (expected {expected})"),
            Self::UnknownMessageKind(k) => write!(f, "unknown message kind 0x{k:02x}"),
            Self::BufferTooShort { needed, got } => write!(f, "need {needed} bytes, got {got}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageKind {
    Midi { status: u8, data1: u8, data2: u8 },
    TMinusTick { tick: i64 },
    AgentSync { agent_id: u32, phase_bits: u32 },
    /// Tempo in milli-BPM (120 BPM = 120_000).
    EnsembleState { tempo_milli_bpm: u32 },
    Heartbeat,
}

/// Wire frame: [version(1) | sequence(4) | tag(1) | payload].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub version: u8,
    pub sequence: u32,
    pub kind: MessageKind,
}

impl Frame {
    pub fn new(sequence: u32, kind: MessageKind) -> Self {
        Frame { version: PROTOCOL_VERSION, sequence, kind }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(20);
        buf.push(self.version);
        buf.extend_from_slice(&self.sequence.to_le_bytes());
        match &self.kind {
            MessageKind::Midi { status, data1, data2 } => {
                buf.push(tag::MIDI);
                buf.extend_from_slice(&[*status, *data1, *data2]);
            }
            MessageKind::TMinusTick { tick } => {
                buf.push(tag::TMINUS_TICK);
                buf.extend_from_slice(&tick.to_le_bytes());
            }
            MessageKind::AgentSync { agent_id, phase_bits } => {
                buf.push(tag::AGENT_SYNC);
                buf.extend_from_slice(&agent_id.to_le_bytes());
                buf.extend_from_slice(&phase_bits.to_le_bytes());
            }
            MessageKind::EnsembleState { tempo_milli_bpm } => {
                buf.push(tag::ENSEMBLE_STATE);
                buf.extend_from_slice(&tempo_milli_bpm.to_le_bytes());
            }
            MessageKind::Heartbeat => { buf.push(tag::HEARTBEAT); }
        }
        buf
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        const HDR: usize = 6;
        if bytes.len() < HDR {
            return Err(ProtocolError::BufferTooShort { needed: HDR, got: bytes.len() });
        }
        let version = bytes[0];
        if version != PROTOCOL_VERSION {
            return Err(ProtocolError::InvalidVersion { expected: PROTOCOL_VERSION, got: version });
        }
        let sequence = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
        let p = &bytes[HDR..];
        let kind = match bytes[5] {
            tag::MIDI => {
                if p.len() < 3 { return Err(ProtocolError::BufferTooShort { needed: 3, got: p.len() }); }
                MessageKind::Midi { status: p[0], data1: p[1], data2: p[2] }
            }
            tag::TMINUS_TICK => {
                if p.len() < 8 { return Err(ProtocolError::BufferTooShort { needed: 8, got: p.len() }); }
                MessageKind::TMinusTick { tick: i64::from_le_bytes([p[0],p[1],p[2],p[3],p[4],p[5],p[6],p[7]]) }
            }
            tag::AGENT_SYNC => {
                if p.len() < 8 { return Err(ProtocolError::BufferTooShort { needed: 8, got: p.len() }); }
                MessageKind::AgentSync {
                    agent_id: u32::from_le_bytes([p[0],p[1],p[2],p[3]]),
                    phase_bits: u32::from_le_bytes([p[4],p[5],p[6],p[7]]),
                }
            }
            tag::ENSEMBLE_STATE => {
                if p.len() < 4 { return Err(ProtocolError::BufferTooShort { needed: 4, got: p.len() }); }
                MessageKind::EnsembleState { tempo_milli_bpm: u32::from_le_bytes([p[0],p[1],p[2],p[3]]) }
            }
            tag::HEARTBEAT => MessageKind::Heartbeat,
            other => return Err(ProtocolError::UnknownMessageKind(other)),
        };
        Ok(Frame { version, sequence, kind })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(kind: MessageKind) -> Frame {
        let f = Frame::new(42, kind);
        Frame::decode(&f.encode()).unwrap()
    }

    #[test] fn round_trip_midi() {
        let k = MessageKind::Midi { status: 0x90, data1: 60, data2: 100 };
        assert_eq!(rt(k.clone()).kind, k);
    }
    #[test] fn round_trip_tminus_positive() {
        let k = MessageKind::TMinusTick { tick: 12345 };
        assert_eq!(rt(k.clone()).kind, k);
    }
    #[test] fn round_trip_tminus_negative() {
        let k = MessageKind::TMinusTick { tick: -9999 };
        assert_eq!(rt(k.clone()).kind, k);
    }
    #[test] fn round_trip_tminus_zero() {
        let k = MessageKind::TMinusTick { tick: 0 };
        assert_eq!(rt(k.clone()).kind, k);
    }
    #[test] fn round_trip_tminus_min_i64() {
        let k = MessageKind::TMinusTick { tick: i64::MIN };
        assert_eq!(rt(k.clone()).kind, k);
    }
    #[test] fn round_trip_agent_sync() {
        let k = MessageKind::AgentSync { agent_id: 7, phase_bits: 0xDEAD_BEEF };
        assert_eq!(rt(k.clone()).kind, k);
    }
    #[test] fn round_trip_ensemble_state() {
        let k = MessageKind::EnsembleState { tempo_milli_bpm: 120_000 };
        assert_eq!(rt(k.clone()).kind, k);
    }
    #[test] fn round_trip_heartbeat() {
        assert_eq!(rt(MessageKind::Heartbeat).kind, MessageKind::Heartbeat);
    }
    #[test] fn sequence_preserved() {
        let f = Frame { version: PROTOCOL_VERSION, sequence: 0xABCD_1234, kind: MessageKind::Heartbeat };
        assert_eq!(Frame::decode(&f.encode()).unwrap().sequence, 0xABCD_1234);
    }
    #[test] fn version_in_header() {
        assert_eq!(Frame::new(0, MessageKind::Heartbeat).encode()[0], PROTOCOL_VERSION);
    }
    #[test] fn decode_empty_err() {
        assert!(matches!(Frame::decode(&[]), Err(ProtocolError::BufferTooShort { .. })));
    }
    #[test] fn decode_short_header_err() {
        assert!(matches!(Frame::decode(&[1,0,0,0,0]), Err(ProtocolError::BufferTooShort { .. })));
    }
    #[test] fn decode_wrong_version_err() {
        let mut b = Frame::new(0, MessageKind::Heartbeat).encode();
        b[0] = 99;
        assert!(matches!(Frame::decode(&b), Err(ProtocolError::InvalidVersion { .. })));
    }
    #[test] fn decode_unknown_kind_err() {
        let mut b = Frame::new(0, MessageKind::Heartbeat).encode();
        b[5] = 0xFF;
        assert!(matches!(Frame::decode(&b), Err(ProtocolError::UnknownMessageKind(0xFF))));
    }
    #[test] fn decode_midi_too_short() {
        let mut b = Frame::new(0, MessageKind::Midi { status: 0x90, data1: 60, data2: 100 }).encode();
        b.pop();
        assert!(matches!(Frame::decode(&b), Err(ProtocolError::BufferTooShort { .. })));
    }
    #[test] fn decode_tminus_too_short() {
        let mut b = Frame::new(0, MessageKind::TMinusTick { tick: 0 }).encode();
        b.pop();
        assert!(matches!(Frame::decode(&b), Err(ProtocolError::BufferTooShort { .. })));
    }
    #[test] fn decode_agent_sync_too_short() {
        let mut b = Frame::new(0, MessageKind::AgentSync { agent_id: 1, phase_bits: 2 }).encode();
        b.pop();
        assert!(matches!(Frame::decode(&b), Err(ProtocolError::BufferTooShort { .. })));
    }
    #[test] fn decode_ensemble_state_too_short() {
        let mut b = Frame::new(0, MessageKind::EnsembleState { tempo_milli_bpm: 120_000 }).encode();
        b.pop();
        assert!(matches!(Frame::decode(&b), Err(ProtocolError::BufferTooShort { .. })));
    }
    #[test] fn error_display_invalid_version() {
        assert!(!ProtocolError::InvalidVersion { expected: 1, got: 2 }.to_string().is_empty());
    }
    #[test] fn error_display_unknown_kind() {
        assert!(!ProtocolError::UnknownMessageKind(0xAB).to_string().is_empty());
    }
    #[test] fn error_display_too_short() {
        assert!(!ProtocolError::BufferTooShort { needed: 10, got: 3 }.to_string().is_empty());
    }
    #[test] fn zero_sequence_round_trip() {
        let f = Frame::new(0, MessageKind::TMinusTick { tick: 1 });
        assert_eq!(Frame::decode(&f.encode()).unwrap(), f);
    }
    #[test] fn max_sequence_round_trip() {
        let f = Frame::new(u32::MAX, MessageKind::TMinusTick { tick: 1 });
        assert_eq!(Frame::decode(&f.encode()).unwrap(), f);
    }
}
