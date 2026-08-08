use std::sync::Arc;

use super::{Db, FileFacts, SourceInput};
use crate::compiler::db::model::{LanguageFeature, LanguageFeatureUse, PackageIssue};
use crate::compiler::db::telemetry::QueryKind;
use crate::compiler::input::GoLanguageVersion;
use crate::parser::{TokenObservation, TokenSpelling};
use crate::source::{TextRange, TextSize};
use crate::token::Token;

pub(super) fn project_language_facts(
    file_version: &str,
    observations: &[TokenObservation<'_>],
    source_len: TextSize,
) -> (Option<GoLanguageVersion>, Arc<[LanguageFeatureUse]>) {
    let file_version = (!file_version.is_empty())
        .then(|| GoLanguageVersion::parse(file_version).ok())
        .flatten();
    let features = observations
        .iter()
        .filter_map(|observation| {
            let token = observation.token();
            if !matches!(token, Token::INT | Token::FLOAT | Token::IMAG) {
                return None;
            }
            let TokenSpelling::Source(spelling) = observation.spelling() else {
                return None;
            };
            if spelling.len() <= 2 {
                return None;
            }
            let feature = if spelling.contains('_') {
                LanguageFeature::NumericLiteralSeparator
            } else if spelling.starts_with("0b") || spelling.starts_with("0B") {
                LanguageFeature::BinaryLiteral
            } else if spelling.starts_with("0o") || spelling.starts_with("0O") {
                LanguageFeature::OctalLiteral
            } else if matches!(token, Token::FLOAT | Token::IMAG)
                && (spelling.starts_with("0x") || spelling.starts_with("0X"))
            {
                LanguageFeature::HexadecimalFloatingPointLiteral
            } else {
                return None;
            };
            let source_limit = source_len.to_usize();
            let start = TextSize::try_from(observation.byte_offset().min(source_limit)).ok()?;
            let end = TextSize::try_from(observation.byte_end().min(source_limit)).ok()?;
            let range = TextRange::new(start, end).ok()?;
            Some(LanguageFeatureUse { feature, range })
        })
        .collect::<Vec<_>>();
    (file_version, features.into())
}

#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn language_version_issues(
    db: &dyn Db,
    source: SourceInput,
    facts: FileFacts<'_>,
) -> Arc<[PackageIssue]> {
    db.query_telemetry()
        .record_query(QueryKind::LanguageVersionCheck);
    let package_version = source.language_version(db);
    let selected = facts
        .file_language_version(db)
        .map_or(package_version, |version| {
            version.max(GoLanguageVersion::FILE_VERSION_FLOOR)
        });
    facts
        .language_features(db)
        .iter()
        .filter(|usage| selected < usage.feature.required_version())
        .map(|usage| PackageIssue::LanguageVersion {
            file: facts.file(db),
            feature: usage.feature,
            selected,
            range: usage.range,
        })
        .collect::<Vec<_>>()
        .into()
}
