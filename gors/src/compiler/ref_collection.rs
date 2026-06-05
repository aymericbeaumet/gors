use super::receiver_type_facts::{ReceiverFieldTypeMap, ReceiverTupleReturnMap, ReceiverTypeMap};

pub(super) type ReachabilityNameSet = std::collections::HashSet<String>;

pub(super) struct RefCollectionContext<'a> {
    pub(super) module_names: &'a ReachabilityNameSet,
    pub(super) item_names: &'a ReachabilityNameSet,
    pub(super) top_level_names: &'a ReachabilityNameSet,
    pub(super) top_level_types: &'a ReceiverTypeMap,
    pub(super) top_level_field_types: &'a ReceiverFieldTypeMap,
    pub(super) top_level_element_types: &'a ReceiverTypeMap,
    pub(super) top_level_return_types: &'a ReceiverTypeMap,
    pub(super) top_level_tuple_return_types: &'a ReceiverTupleReturnMap,
}
