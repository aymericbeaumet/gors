//! Canonical encoding of complete runtime operation contracts.

use super::RuntimeOp;
use crate::encoding::CanonicalEncoder;

impl RuntimeOp {
    pub(crate) fn encode(self, encoder: &mut CanonicalEncoder) {
        encoder.u16(self.id().get());
        encoder.text(self.symbol());
        self.signature().encode(encoder);
        self.effects().encode(encoder);
        let requirements = self.required_capabilities();
        encoder.count(requirements.len());
        for requirement in requirements {
            encoder.u16(requirement.canonical_tag());
        }
    }
}
