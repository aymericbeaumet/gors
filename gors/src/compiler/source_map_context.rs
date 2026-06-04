use std::cell::RefCell;

use crate::mapping::SourceMapTracker;
use crate::token;

thread_local! {
    static TRACKER: RefCell<SourceMapTracker> = RefCell::new(SourceMapTracker::new());
}

pub(super) fn record_mapping(pos: &token::Position, name: Option<&str>) {
    let source = if pos.file.is_empty() {
        None
    } else if pos.file.starts_with('/') {
        Some(pos.file.to_string())
    } else {
        Some(format!("{}/{}", pos.directory, pos.file))
    };
    TRACKER.with(|tracker| {
        tracker
            .borrow_mut()
            .record_for_source(source, pos.line as u32, pos.column as u32, name);
    });
}

pub(super) fn start(go_file: &str, rust_file: &str, go_source: Option<&str>) {
    TRACKER.with(|tracker| {
        tracker.borrow_mut().start(go_file, rust_file, go_source);
    });
}

pub(super) fn start_many(sources: Vec<(String, Option<String>)>, rust_file: &str) {
    TRACKER.with(|tracker| {
        tracker.borrow_mut().start_many(sources, rust_file);
    });
}

pub(super) fn build_source_map(rust_source: &str) -> sourcemap::SourceMap {
    TRACKER.with(|tracker| tracker.borrow().build_source_map(rust_source))
}

pub(super) fn clear() {
    TRACKER.with(|tracker| {
        tracker.borrow_mut().clear();
    });
}
