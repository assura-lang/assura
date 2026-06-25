// ===========================================================================
// T073: FMT.4 Codec dispatch (magic-byte routing)
// ===========================================================================

/// Routes decoding to the appropriate codec based on magic bytes.
///
/// Error codes (via assura-types):
/// - A33001: unknown magic bytes
/// - A33002: ambiguous magic bytes (multiple codecs match)
/// - A33003: codec not registered
#[derive(Debug, Clone)]
pub struct CodecDispatcher {
    codecs: Vec<CodecEntry>,
}

#[derive(Debug, Clone)]
pub struct CodecEntry {
    pub name: String,
    pub magic_bytes: Vec<u8>,
    pub magic_offset: usize,
}

impl CodecDispatcher {
    pub fn new() -> Self {
        Self { codecs: Vec::new() }
    }

    pub fn register(&mut self, name: String, magic_bytes: Vec<u8>, offset: usize) {
        self.codecs.push(CodecEntry {
            name,
            magic_bytes,
            magic_offset: offset,
        });
    }

    /// Dispatch: find the codec matching the given data prefix.
    pub fn dispatch(&self, data: &[u8]) -> DispatchResult {
        let mut matches: Vec<&CodecEntry> = Vec::new();
        for codec in &self.codecs {
            let end = codec.magic_offset + codec.magic_bytes.len();
            if data.len() >= end && data[codec.magic_offset..end] == codec.magic_bytes {
                matches.push(codec);
            }
        }
        match matches.len() {
            0 => DispatchResult::Unknown,
            1 => DispatchResult::Matched(matches[0].name.clone()),
            _ => DispatchResult::Ambiguous(matches.iter().map(|c| c.name.clone()).collect()),
        }
    }

    /// Check for ambiguous registrations (overlapping magic bytes).
    pub fn check_ambiguity(&self) -> Vec<(String, String)> {
        let mut conflicts = Vec::new();
        for i in 0..self.codecs.len() {
            for j in (i + 1)..self.codecs.len() {
                let a = &self.codecs[i];
                let b = &self.codecs[j];
                if a.magic_offset == b.magic_offset && a.magic_bytes == b.magic_bytes {
                    conflicts.push((a.name.clone(), b.name.clone()));
                }
            }
        }
        conflicts
    }

    pub fn codec_count(&self) -> usize {
        self.codecs.len()
    }
}

impl Default for CodecDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DispatchResult {
    Matched(String),
    Unknown,
    Ambiguous(Vec<String>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_new_is_empty() {
        let cd = CodecDispatcher::new();
        assert_eq!(cd.codec_count(), 0);
    }

    #[test]
    fn codec_default_is_empty() {
        let cd = CodecDispatcher::default();
        assert_eq!(cd.codec_count(), 0);
    }

    #[test]
    fn codec_register_increases_count() {
        let mut cd = CodecDispatcher::new();
        cd.register("png".into(), vec![0x89, 0x50, 0x4E, 0x47], 0);
        assert_eq!(cd.codec_count(), 1);
    }

    #[test]
    fn codec_dispatch_matches() {
        let mut cd = CodecDispatcher::new();
        cd.register("png".into(), vec![0x89, 0x50], 0);
        let data = vec![0x89, 0x50, 0x4E, 0x47, 0x00];
        assert_eq!(cd.dispatch(&data), DispatchResult::Matched("png".into()));
    }

    #[test]
    fn codec_dispatch_unknown() {
        let mut cd = CodecDispatcher::new();
        cd.register("png".into(), vec![0x89, 0x50], 0);
        let data = vec![0xFF, 0xD8, 0xFF]; // JPEG magic
        assert_eq!(cd.dispatch(&data), DispatchResult::Unknown);
    }

    #[test]
    fn codec_dispatch_ambiguous() {
        let mut cd = CodecDispatcher::new();
        cd.register("a".into(), vec![0xAA], 0);
        cd.register("b".into(), vec![0xAA], 0);
        let data = vec![0xAA, 0x00];
        assert_eq!(
            cd.dispatch(&data),
            DispatchResult::Ambiguous(vec!["a".into(), "b".into()])
        );
    }

    #[test]
    fn codec_dispatch_with_offset() {
        let mut cd = CodecDispatcher::new();
        cd.register("custom".into(), vec![0xBE, 0xEF], 2);
        let data = vec![0x00, 0x00, 0xBE, 0xEF, 0x00];
        assert_eq!(cd.dispatch(&data), DispatchResult::Matched("custom".into()));
    }

    #[test]
    fn codec_dispatch_data_too_short() {
        let mut cd = CodecDispatcher::new();
        cd.register("wide".into(), vec![0x01, 0x02, 0x03, 0x04], 0);
        let data = vec![0x01, 0x02]; // shorter than magic
        assert_eq!(cd.dispatch(&data), DispatchResult::Unknown);
    }

    #[test]
    fn codec_check_ambiguity_detects_conflict() {
        let mut cd = CodecDispatcher::new();
        cd.register("a".into(), vec![0xFF], 0);
        cd.register("b".into(), vec![0xFF], 0);
        let conflicts = cd.check_ambiguity();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0], ("a".into(), "b".into()));
    }

    #[test]
    fn codec_check_ambiguity_no_conflict() {
        let mut cd = CodecDispatcher::new();
        cd.register("a".into(), vec![0xAA], 0);
        cd.register("b".into(), vec![0xBB], 0);
        assert!(cd.check_ambiguity().is_empty());
    }
}
