#![allow(
    dead_code,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals
)]

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Once};

pub type any = dyn Any;
pub type r#bool = std::primitive::bool;
pub type byte = u8;
pub type complex64 = Complex64;
pub type complex128 = Complex128;
pub type float32 = f32;
pub type float64 = f64;
pub type int = isize;
pub type int8 = i8;
pub type int16 = i16;
pub type int32 = i32;
pub type int64 = i64;
pub type rune = i32;
pub type string = std::string::String;
pub type uint = usize;
pub type uint8 = u8;
pub type uint16 = u16;
pub type uint32 = u32;
pub type uint64 = u64;
pub type uintptr = usize;

pub trait comparable {}

impl<T: Eq> comparable for T {}

#[derive(Clone)]
enum GorsInterfaceKeyKind {
    Nil,
    Pointer {
        type_name: &'static str,
        data: usize,
        field_key: usize,
    },
    Comparable {
        type_name: &'static str,
        value: Arc<dyn Any + Send + Sync>,
        equals: fn(&dyn Any, &dyn Any) -> bool,
    },
    NonComparable {
        type_name: &'static str,
    },
}

/// An owned key for the dynamic value stored in a Go interface.
///
/// Comparable values retain their concrete value for equality. Their hash is
/// deliberately coarse (dynamic type only): equal values still hash equally,
/// and Go comparability does not require the concrete Rust type to implement
/// [`Hash`]. Non-comparable values are represented without panicking so both Go
/// comparison operands can finish evaluating before equality raises the panic.
#[derive(Clone)]
pub struct GorsInterfaceKey {
    kind: GorsInterfaceKeyKind,
}

impl Default for GorsInterfaceKey {
    fn default() -> Self {
        Self {
            kind: GorsInterfaceKeyKind::Nil,
        }
    }
}

impl std::fmt::Debug for GorsInterfaceKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            GorsInterfaceKeyKind::Nil => formatter.write_str("GorsInterfaceKey::Nil"),
            GorsInterfaceKeyKind::Pointer {
                type_name,
                data,
                field_key,
            } => formatter
                .debug_struct("GorsInterfaceKey::Pointer")
                .field("type_name", type_name)
                .field("data", data)
                .field("field_key", field_key)
                .finish(),
            GorsInterfaceKeyKind::Comparable { type_name, .. } => formatter
                .debug_struct("GorsInterfaceKey::Comparable")
                .field("type_name", type_name)
                .finish_non_exhaustive(),
            GorsInterfaceKeyKind::NonComparable { type_name } => formatter
                .debug_struct("GorsInterfaceKey::NonComparable")
                .field("type_name", type_name)
                .finish(),
        }
    }
}

impl PartialEq for GorsInterfaceKey {
    fn eq(&self, other: &Self) -> bool {
        match (&self.kind, &other.kind) {
            (GorsInterfaceKeyKind::Nil, GorsInterfaceKeyKind::Nil) => true,
            (
                GorsInterfaceKeyKind::Pointer {
                    type_name: left_type,
                    data: left_data,
                    field_key: left_field_key,
                },
                GorsInterfaceKeyKind::Pointer {
                    type_name: right_type,
                    data: right_data,
                    field_key: right_field_key,
                },
            ) => {
                left_type == right_type
                    && left_data == right_data
                    && left_field_key == right_field_key
            }
            (
                GorsInterfaceKeyKind::Comparable {
                    type_name: left_type,
                    value: left_value,
                    equals: left_equals,
                },
                GorsInterfaceKeyKind::Comparable {
                    type_name: right_type,
                    value: right_value,
                    ..
                },
            ) => left_type == right_type && left_equals(left_value.as_ref(), right_value.as_ref()),
            (
                GorsInterfaceKeyKind::NonComparable {
                    type_name: left_type,
                },
                GorsInterfaceKeyKind::NonComparable {
                    type_name: right_type,
                },
            ) if left_type == right_type => {
                panic_value(format!("comparing uncomparable type {left_type}"))
            }
            (
                GorsInterfaceKeyKind::NonComparable {
                    type_name: left_type,
                },
                GorsInterfaceKeyKind::Comparable {
                    type_name: right_type,
                    ..
                },
            )
            | (
                GorsInterfaceKeyKind::Comparable {
                    type_name: left_type,
                    ..
                },
                GorsInterfaceKeyKind::NonComparable {
                    type_name: right_type,
                },
            ) if left_type == right_type => {
                panic_value(format!("comparing uncomparable type {left_type}"))
            }
            _ => false,
        }
    }
}

impl Eq for GorsInterfaceKey {}

impl Hash for GorsInterfaceKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match &self.kind {
            GorsInterfaceKeyKind::Nil => 0_u8.hash(state),
            GorsInterfaceKeyKind::Pointer {
                type_name,
                data,
                field_key,
            } => {
                1_u8.hash(state);
                type_name.hash(state);
                data.hash(state);
                field_key.hash(state);
            }
            GorsInterfaceKeyKind::Comparable { type_name, .. } => {
                2_u8.hash(state);
                type_name.hash(state);
            }
            GorsInterfaceKeyKind::NonComparable { type_name } => {
                panic_value(format!("hash of unhashable type {type_name}"))
            }
        }
    }
}

impl GorsInterfaceKey {
    pub fn nil() -> Self {
        Self::default()
    }

    pub fn for_ptr<T>(ptr: *const ()) -> Self {
        Self {
            kind: GorsInterfaceKeyKind::Pointer {
                type_name: std::any::type_name::<T>(),
                data: ptr as usize,
                field_key: 0,
            },
        }
    }

    pub fn for_projected_ptr<T>(owner: *const (), field_key: usize) -> Self {
        Self {
            kind: GorsInterfaceKeyKind::Pointer {
                type_name: std::any::type_name::<T>(),
                data: owner as usize,
                field_key,
            },
        }
    }

    pub fn for_comparable<T>(value: &T) -> Self
    where
        T: Any + Clone + PartialEq + Send + Sync,
    {
        fn equal_values<T: Any + PartialEq>(left: &dyn Any, right: &dyn Any) -> bool {
            left.downcast_ref::<T>()
                .zip(right.downcast_ref::<T>())
                .is_some_and(|(left, right)| left == right)
        }

        Self {
            kind: GorsInterfaceKeyKind::Comparable {
                type_name: std::any::type_name::<T>(),
                value: Arc::new(value.clone()),
                equals: equal_values::<T>,
            },
        }
    }

    pub fn non_comparable<T: ?Sized>() -> Self {
        Self {
            kind: GorsInterfaceKeyKind::NonComparable {
                type_name: std::any::type_name::<T>(),
            },
        }
    }

    fn is_comparable(&self) -> bool {
        !matches!(self.kind, GorsInterfaceKeyKind::NonComparable { .. })
    }
}

/// Go map values are small, nil-capable references to shared map data.
///
/// Keeping the optional allocation outside the mutex makes the zero value a
/// true nil map while cloning a non-nil value only clones the shared handle.
pub struct GorsMap<K, V> {
    inner: Option<Arc<Mutex<HashMap<K, V>>>>,
}

impl<K, V> Clone for GorsMap<K, V> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<K, V> Default for GorsMap<K, V> {
    fn default() -> Self {
        Self { inner: None }
    }
}

impl<K, V> GorsMap<K, V> {
    pub fn new() -> Self {
        Self {
            inner: Some(Arc::new(Mutex::new(HashMap::new()))),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Some(Arc::new(Mutex::new(HashMap::with_capacity(capacity)))),
        }
    }

    pub fn is_nil(&self) -> bool {
        self.inner.is_none()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.as_ref().is_none_or(|inner| {
            inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        })
    }

    pub fn len(&self) -> usize {
        self.inner.as_ref().map_or(0, |inner| {
            inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len()
        })
    }

    pub fn capacity(&self) -> usize {
        self.inner.as_ref().map_or(0, |inner| {
            inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .capacity()
        })
    }

    pub fn get_with<R>(&self, key: &K, read: impl FnOnce(Option<&V>) -> R) -> R
    where
        K: Eq + Hash,
    {
        match &self.inner {
            Some(inner) => {
                let map = inner
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                read(map.get(key))
            }
            None => read(None),
        }
    }

    pub fn collect_entries<R>(&self, mut collect: impl FnMut(&K, &V) -> R) -> Vec<R> {
        self.inner.as_ref().map_or_else(Vec::new, |inner| {
            let map = inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            map.iter().map(|(key, value)| collect(key, value)).collect()
        })
    }

    pub fn insert(&self, key: K, value: V) -> Option<V>
    where
        K: Eq + Hash,
    {
        let Some(inner) = &self.inner else {
            panic_value("assignment to entry in nil map");
        };
        inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key, value)
    }

    pub fn update_or_insert_with(
        &self,
        key: K,
        default: impl FnOnce() -> V,
        update: impl FnOnce(&mut V),
    ) where
        K: Eq + Hash,
    {
        let Some(inner) = &self.inner else {
            panic_value("assignment to entry in nil map");
        };
        let mut map = inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        update(map.entry(key).or_insert_with(default));
    }

    pub fn delete(&self, key: &K)
    where
        K: Eq + Hash,
    {
        if let Some(inner) = &self.inner {
            let mut map = inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            map.remove(key);
        }
    }

    pub fn clear(&self) {
        if let Some(inner) = &self.inner {
            let mut map = inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            map.clear();
        }
    }

    pub fn deep_clone(&self) -> Self
    where
        K: Clone,
        V: Clone,
    {
        self.inner.as_ref().map_or_else(Self::default, |inner| {
            let map = inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            Self {
                inner: Some(Arc::new(Mutex::new(map.clone()))),
            }
        })
    }
}

impl<K, V, const N: usize> From<[(K, V); N]> for GorsMap<K, V>
where
    K: Eq + Hash,
{
    fn from(entries: [(K, V); N]) -> Self {
        Self {
            inner: Some(Arc::new(Mutex::new(HashMap::from(entries)))),
        }
    }
}

impl<K, V> FromIterator<(K, V)> for GorsMap<K, V>
where
    K: Eq + Hash,
{
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        Self {
            inner: Some(Arc::new(Mutex::new(iter.into_iter().collect()))),
        }
    }
}

pub trait GorsAnyComparable: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn clone_comparable_any(&self) -> Box<dyn Any>;
    fn clone_comparable_any_send(&self) -> Box<dyn Any + Send>;
    fn clone_comparable_any_send_sync(&self) -> Box<dyn Any + Send + Sync>;
    fn clone_raw_any(&self) -> Box<dyn Any>;
    fn clone_raw_any_send(&self) -> Box<dyn Any + Send>;
    fn clone_raw_any_send_sync(&self) -> Box<dyn Any + Send + Sync>;
    fn eq_any(&self, other: &dyn Any) -> bool;
}

#[derive(Clone)]
pub struct GorsComparableAny<T: Any + Clone + PartialEq + Send + Sync>(pub T);

impl<T> GorsAnyComparable for GorsComparableAny<T>
where
    T: Any + Clone + PartialEq + Send + Sync,
{
    fn as_any(&self) -> &dyn Any {
        &self.0
    }

    fn clone_comparable_any(&self) -> Box<dyn Any> {
        Box::new(Box::new(Self(self.0.clone())) as Box<dyn GorsAnyComparable>) as Box<dyn Any>
    }

    fn clone_comparable_any_send(&self) -> Box<dyn Any + Send> {
        Box::new(Box::new(Self(self.0.clone())) as Box<dyn GorsAnyComparable>)
            as Box<dyn Any + Send>
    }

    fn clone_comparable_any_send_sync(&self) -> Box<dyn Any + Send + Sync> {
        Box::new(Box::new(Self(self.0.clone())) as Box<dyn GorsAnyComparable>)
            as Box<dyn Any + Send + Sync>
    }

    fn clone_raw_any(&self) -> Box<dyn Any> {
        Box::new(self.0.clone())
    }

    fn clone_raw_any_send(&self) -> Box<dyn Any + Send> {
        Box::new(self.0.clone())
    }

    fn clone_raw_any_send_sync(&self) -> Box<dyn Any + Send + Sync> {
        Box::new(self.0.clone())
    }

    fn eq_any(&self, other: &dyn Any) -> bool {
        erased_any_payload(other)
            .downcast_ref::<T>()
            .is_some_and(|other| self.0 == *other)
    }
}

pub fn box_any_comparable<T>(value: T) -> Box<dyn Any>
where
    T: Any + Clone + PartialEq + Send + Sync,
{
    Box::new(Box::new(GorsComparableAny(value)) as Box<dyn GorsAnyComparable>) as Box<dyn Any>
}

pub fn box_any_comparable_send_sync<T>(value: T) -> Box<dyn Any + Send + Sync>
where
    T: Any + Clone + PartialEq + Send + Sync,
{
    Box::new(Box::new(GorsComparableAny(value)) as Box<dyn GorsAnyComparable>)
        as Box<dyn Any + Send + Sync>
}

pub trait GorsAnyLocalComparable {
    fn as_any(&self) -> &dyn Any;
    fn clone_comparable_any(&self) -> Box<dyn Any>;
    fn eq_any(&self, other: &dyn Any) -> bool;
}

#[derive(Clone)]
pub struct GorsLocalComparableAny<T: Any + Clone + PartialEq>(pub T);

impl<T> GorsAnyLocalComparable for GorsLocalComparableAny<T>
where
    T: Any + Clone + PartialEq,
{
    fn as_any(&self) -> &dyn Any {
        &self.0
    }

    fn clone_comparable_any(&self) -> Box<dyn Any> {
        Box::new(Box::new(Self(self.0.clone())) as Box<dyn GorsAnyLocalComparable>) as Box<dyn Any>
    }

    fn eq_any(&self, other: &dyn Any) -> bool {
        erased_any_payload(other)
            .downcast_ref::<T>()
            .is_some_and(|other| self.0 == *other)
    }
}

pub fn box_any_local_comparable<T>(value: T) -> Box<dyn Any>
where
    T: Any + Clone + PartialEq,
{
    Box::new(Box::new(GorsLocalComparableAny(value)) as Box<dyn GorsAnyLocalComparable>)
        as Box<dyn Any>
}

pub trait GorsAnyClone: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn clone_erased_any(&self) -> Box<dyn Any>;
    fn clone_erased_any_send(&self) -> Box<dyn Any + Send>;
    fn clone_erased_any_send_sync(&self) -> Box<dyn Any + Send + Sync>;
}

#[derive(Clone)]
pub struct GorsCloneAny<T: Any + Clone + Send + Sync>(pub T);

impl<T> GorsAnyClone for GorsCloneAny<T>
where
    T: Any + Clone + Send + Sync,
{
    fn as_any(&self) -> &dyn Any {
        &self.0
    }

    fn clone_erased_any(&self) -> Box<dyn Any> {
        Box::new(Box::new(Self(self.0.clone())) as Box<dyn GorsAnyClone>) as Box<dyn Any>
    }

    fn clone_erased_any_send(&self) -> Box<dyn Any + Send> {
        Box::new(Box::new(Self(self.0.clone())) as Box<dyn GorsAnyClone>) as Box<dyn Any + Send>
    }

    fn clone_erased_any_send_sync(&self) -> Box<dyn Any + Send + Sync> {
        Box::new(Box::new(Self(self.0.clone())) as Box<dyn GorsAnyClone>)
            as Box<dyn Any + Send + Sync>
    }
}

pub fn box_any_clone<T>(value: T) -> Box<dyn Any>
where
    T: Any + Clone + Send + Sync,
{
    Box::new(Box::new(GorsCloneAny(value)) as Box<dyn GorsAnyClone>) as Box<dyn Any>
}

pub fn box_any_clone_send_sync<T>(value: T) -> Box<dyn Any + Send + Sync>
where
    T: Any + Clone + Send + Sync,
{
    Box::new(Box::new(GorsCloneAny(value)) as Box<dyn GorsAnyClone>) as Box<dyn Any + Send + Sync>
}

pub trait GorsAnyLocalClone {
    fn as_any(&self) -> &dyn Any;
    fn clone_erased_any(&self) -> Box<dyn Any>;
}

#[derive(Clone)]
pub struct GorsLocalCloneAny<T: Any + Clone>(pub T);

impl<T> GorsAnyLocalClone for GorsLocalCloneAny<T>
where
    T: Any + Clone,
{
    fn as_any(&self) -> &dyn Any {
        &self.0
    }

    fn clone_erased_any(&self) -> Box<dyn Any> {
        Box::new(Box::new(Self(self.0.clone())) as Box<dyn GorsAnyLocalClone>) as Box<dyn Any>
    }
}

pub fn box_any_local_clone<T>(value: T) -> Box<dyn Any>
where
    T: Any + Clone,
{
    Box::new(Box::new(GorsLocalCloneAny(value)) as Box<dyn GorsAnyLocalClone>) as Box<dyn Any>
}

fn comparable_any(value: &dyn Any) -> Option<&dyn GorsAnyComparable> {
    value
        .downcast_ref::<Box<dyn GorsAnyComparable>>()
        .map(|value| &**value)
}

fn local_comparable_any(value: &dyn Any) -> Option<&dyn GorsAnyLocalComparable> {
    value
        .downcast_ref::<Box<dyn GorsAnyLocalComparable>>()
        .map(|value| &**value)
}

fn clone_only_any(value: &dyn Any) -> Option<&dyn GorsAnyClone> {
    value
        .downcast_ref::<Box<dyn GorsAnyClone>>()
        .map(|value| &**value)
}

fn local_clone_only_any(value: &dyn Any) -> Option<&dyn GorsAnyLocalClone> {
    value
        .downcast_ref::<Box<dyn GorsAnyLocalClone>>()
        .map(|value| &**value)
}

fn erased_any_payload(mut value: &dyn Any) -> &dyn Any {
    loop {
        if let Some(wrapper) = comparable_any(value) {
            value = wrapper.as_any();
            continue;
        }
        if let Some(wrapper) = local_comparable_any(value) {
            value = wrapper.as_any();
            continue;
        }
        if let Some(wrapper) = clone_only_any(value) {
            value = wrapper.as_any();
            continue;
        }
        if let Some(wrapper) = local_clone_only_any(value) {
            value = wrapper.as_any();
            continue;
        }
        if let Some(boxed) = value.downcast_ref::<Box<dyn Any>>() {
            value = boxed.as_ref();
            continue;
        }
        if let Some(boxed) = value.downcast_ref::<Box<dyn Any + Send>>() {
            value = boxed.as_ref();
            continue;
        }
        if let Some(boxed) = value.downcast_ref::<Box<dyn Any + Send + Sync>>() {
            value = boxed.as_ref();
            continue;
        }
        if let Some(error) = value.downcast_ref::<Box<dyn error>>() {
            if let Some(payload) = error.__gors_as_any() {
                value = payload;
                continue;
            }
            return &();
        }
        return value;
    }
}

pub fn any_is<T: Any>(value: &dyn Any) -> bool {
    erased_any_payload(value).is::<T>()
}

pub fn any_downcast_ref<T: Any>(value: &dyn Any) -> Option<&T> {
    erased_any_payload(value).downcast_ref::<T>()
}

/// Return the dynamic Go value identity carried by an erased runtime value.
///
/// Compiler-generated `any` values may be wrapped to retain clone or
/// comparability capabilities. Reflection observes the wrapped payload type,
/// never the implementation wrapper. The unit payload is the runtime's nil
/// interface sentinel and therefore has no dynamic type.
pub fn any_dynamic_type_id(value: &dyn Any) -> Option<TypeId> {
    let payload = erased_any_payload(value);
    (!payload.is::<()>()).then(|| payload.type_id())
}

pub trait error: Send + Sync {
    fn __gors_as_any(&self) -> Option<&dyn Any>;
    fn __gors_interface_key(&self) -> GorsInterfaceKey;
    fn __gors_clone_box(&self) -> Box<dyn error>;
    fn Error(&self) -> std::string::String;
}

#[derive(Clone, Default)]
pub struct __GorsNooperror;

impl error for __GorsNooperror {
    fn __gors_as_any(&self) -> Option<&dyn Any> {
        None
    }

    fn __gors_interface_key(&self) -> GorsInterfaceKey {
        GorsInterfaceKey::nil()
    }

    fn __gors_clone_box(&self) -> Box<dyn error> {
        Box::new(Self)
    }

    fn Error(&self) -> std::string::String {
        std::string::String::new()
    }
}

#[derive(Clone, Default, Eq, PartialEq)]
pub struct __GorsStringError(pub std::string::String);

impl error for __GorsStringError {
    fn __gors_as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }

    fn __gors_interface_key(&self) -> GorsInterfaceKey {
        GorsInterfaceKey::for_comparable(self)
    }

    fn __gors_clone_box(&self) -> Box<dyn error> {
        Box::new(self.clone())
    }

    fn Error(&self) -> std::string::String {
        self.0.clone()
    }
}

impl Default for Box<dyn error> {
    fn default() -> Self {
        Box::new(__GorsNooperror)
    }
}

impl Clone for Box<dyn error> {
    fn clone(&self) -> Self {
        error::__gors_clone_box(&**self)
    }
}

impl PartialEq for Box<dyn error> {
    fn eq(&self, other: &Self) -> bool {
        self.__gors_interface_key() == other.__gors_interface_key()
    }
}

impl error for Box<dyn error> {
    fn __gors_as_any(&self) -> Option<&dyn Any> {
        (**self).__gors_as_any()
    }

    fn __gors_interface_key(&self) -> GorsInterfaceKey {
        (**self).__gors_interface_key()
    }

    fn __gors_clone_box(&self) -> Box<dyn error> {
        (**self).__gors_clone_box()
    }

    fn Error(&self) -> std::string::String {
        (**self).Error()
    }
}

pub fn error_string(value: &dyn error) -> std::string::String {
    if value.__gors_as_any().is_none() {
        std::string::String::new()
    } else {
        value.Error()
    }
}

pub fn clone_any(value: &dyn Any) -> Box<dyn Any> {
    clone_any_ref(value)
}

pub fn clone_any_ref(value: &dyn Any) -> Box<dyn Any> {
    if let Some(value) = comparable_any(value) {
        return value.clone_comparable_any();
    }
    if let Some(value) = local_comparable_any(value) {
        return value.clone_comparable_any();
    }
    if let Some(value) = clone_only_any(value) {
        return value.clone_erased_any();
    }
    if let Some(value) = local_clone_only_any(value) {
        return value.clone_erased_any();
    }
    if let Some(v) = value.downcast_ref::<Box<dyn Any>>() {
        return clone_any_ref(v.as_ref());
    }
    if let Some(v) = value.downcast_ref::<Box<dyn Any + Send>>() {
        return clone_any_ref(v.as_ref());
    }
    if let Some(v) = value.downcast_ref::<Box<dyn Any + Send + Sync>>() {
        return clone_any_ref(v.as_ref());
    }

    macro_rules! clone_if {
        ($ty:ty) => {
            if let Some(v) = value.downcast_ref::<$ty>() {
                return Box::new(v.clone()) as Box<dyn Any>;
            }
        };
    }

    clone_if!(GorsReflectValue);
    clone_if!(std::string::String);
    clone_if!(&'static str);
    clone_if!(bool);
    clone_if!(isize);
    clone_if!(i8);
    clone_if!(i16);
    clone_if!(i32);
    clone_if!(i64);
    clone_if!(usize);
    clone_if!(u8);
    clone_if!(u16);
    clone_if!(u32);
    clone_if!(u64);
    clone_if!(f32);
    clone_if!(f64);
    clone_if!(Vec<u8>);
    clone_if!(Vec<std::string::String>);
    clone_if!(Box<dyn error>);

    Box::new(())
}

pub fn clone_any_send_ref(value: &dyn Any) -> Box<dyn Any + Send> {
    if let Some(value) = comparable_any(value) {
        return value.clone_comparable_any_send();
    }
    if let Some(value) = clone_only_any(value) {
        return value.clone_erased_any_send();
    }
    if let Some(v) = value.downcast_ref::<Box<dyn Any>>() {
        return clone_any_send_ref(v.as_ref());
    }
    if let Some(v) = value.downcast_ref::<Box<dyn Any + Send>>() {
        return clone_any_send_ref(v.as_ref());
    }
    if let Some(v) = value.downcast_ref::<Box<dyn Any + Send + Sync>>() {
        return clone_any_send_ref(v.as_ref());
    }

    macro_rules! clone_if {
        ($ty:ty) => {
            if let Some(v) = value.downcast_ref::<$ty>() {
                return Box::new(v.clone()) as Box<dyn Any + Send>;
            }
        };
    }

    clone_if!(GorsReflectValue);
    clone_if!(std::string::String);
    clone_if!(&'static str);
    clone_if!(bool);
    clone_if!(isize);
    clone_if!(i8);
    clone_if!(i16);
    clone_if!(i32);
    clone_if!(i64);
    clone_if!(usize);
    clone_if!(u8);
    clone_if!(u16);
    clone_if!(u32);
    clone_if!(u64);
    clone_if!(f32);
    clone_if!(f64);
    clone_if!(Vec<u8>);
    clone_if!(Vec<std::string::String>);
    clone_if!(Box<dyn error>);

    Box::new(())
}

pub fn clone_any_send_sync(value: &dyn Any) -> Box<dyn Any + Send + Sync> {
    if let Some(value) = comparable_any(value) {
        return value.clone_comparable_any_send_sync();
    }
    if let Some(value) = clone_only_any(value) {
        return value.clone_erased_any_send_sync();
    }
    if let Some(v) = value.downcast_ref::<Box<dyn Any>>() {
        return clone_any_send_sync(v.as_ref());
    }
    if let Some(v) = value.downcast_ref::<Box<dyn Any + Send>>() {
        return clone_any_send_sync(v.as_ref());
    }
    if let Some(v) = value.downcast_ref::<Box<dyn Any + Send + Sync>>() {
        return clone_any_send_sync(v.as_ref());
    }

    macro_rules! clone_if {
        ($ty:ty) => {
            if let Some(v) = value.downcast_ref::<$ty>() {
                return Box::new(v.clone()) as Box<dyn Any + Send + Sync>;
            }
        };
    }

    clone_if!(GorsReflectValue);
    clone_if!(std::string::String);
    clone_if!(&'static str);
    clone_if!(bool);
    clone_if!(isize);
    clone_if!(i8);
    clone_if!(i16);
    clone_if!(i32);
    clone_if!(i64);
    clone_if!(usize);
    clone_if!(u8);
    clone_if!(u16);
    clone_if!(u32);
    clone_if!(u64);
    clone_if!(f32);
    clone_if!(f64);
    clone_if!(Vec<u8>);
    clone_if!(Vec<std::string::String>);
    clone_if!(Box<dyn error>);

    Box::new(())
}

trait GorsReflectOps: Send + Sync {
    fn kind(&self) -> __GorsReflectKind;
    fn len(&self) -> isize;
    fn swap(&mut self, i: isize, j: isize);
}

#[derive(Clone)]
pub struct GorsReflectValue {
    ops: Arc<Mutex<Box<dyn GorsReflectOps>>>,
}

pub type GorsReflectSwapper = Arc<Mutex<Option<Arc<dyn Fn(isize, isize) + Send + Sync>>>>;

impl GorsReflectValue {
    pub fn slice<S: GorsReflectSliceTarget + 'static>(slice: Arc<Mutex<S>>) -> Self {
        Self {
            ops: Arc::new(Mutex::new(Box::new(GorsReflectSlice { slice }))),
        }
    }

    pub fn kind(&self) -> __GorsReflectKind {
        lock_reflect_ops(&self.ops).kind()
    }

    pub fn len(&self) -> isize {
        lock_reflect_ops(&self.ops).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn swap(&self, i: isize, j: isize) {
        lock_reflect_ops(&self.ops).swap(i, j);
    }
}

pub trait GorsReflectSliceTarget: Send {
    fn __gors_slice_len(&self) -> usize;
    fn __gors_slice_swap(&mut self, i: usize, j: usize);
}

impl<T: Send> GorsReflectSliceTarget for Vec<T> {
    fn __gors_slice_len(&self) -> usize {
        self.len()
    }

    fn __gors_slice_swap(&mut self, i: usize, j: usize) {
        self.swap(i, j);
    }
}

impl<T: Send> GorsReflectSliceTarget for GorsSliceStorage<T> {
    fn __gors_slice_len(&self) -> usize {
        self.len()
    }

    fn __gors_slice_swap(&mut self, i: usize, j: usize) {
        self.visible_mut().swap(i, j);
    }
}

struct GorsReflectSlice<S> {
    slice: Arc<Mutex<S>>,
}

impl<S: GorsReflectSliceTarget + 'static> GorsReflectOps for GorsReflectSlice<S> {
    fn kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Slice
    }

    fn len(&self) -> isize {
        self.slice
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .__gors_slice_len() as isize
    }

    fn swap(&mut self, i: isize, j: isize) {
        let i =
            usize::try_from(i).unwrap_or_else(|_| panic_value("reflect: slice index out of range"));
        let j =
            usize::try_from(j).unwrap_or_else(|_| panic_value("reflect: slice index out of range"));
        let mut slice = self
            .slice
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if i >= slice.__gors_slice_len() || j >= slice.__gors_slice_len() {
            panic_value("reflect: slice index out of range");
        }
        slice.__gors_slice_swap(i, j);
    }
}

fn lock_reflect_ops(
    ops: &Arc<Mutex<Box<dyn GorsReflectOps>>>,
) -> MutexGuard<'_, Box<dyn GorsReflectOps>> {
    ops.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn reflect_slice_any<S: GorsReflectSliceTarget + 'static>(
    slice: Arc<Mutex<S>>,
) -> Box<dyn Any> {
    Box::new(GorsReflectValue::slice(slice)) as Box<dyn Any>
}

pub fn reflect_value_kind(value: &dyn Any) -> __GorsReflectKind {
    let value = erased_any_payload(value);
    if let Some(value) = value.downcast_ref::<GorsReflectValue>() {
        return value.kind();
    }
    reflect_kind_of_any(value)
}

pub fn reflect_value_len(value: &dyn Any) -> isize {
    let value = erased_any_payload(value);
    if let Some(value) = value.downcast_ref::<GorsReflectValue>() {
        return value.len();
    }
    macro_rules! len_if_slice_storage {
        ($ty:ty) => {
            if let Some(value) = value.downcast_ref::<Vec<$ty>>() {
                return value.len() as isize;
            }
            if let Some(value) = value.downcast_ref::<GorsSliceStorage<$ty>>() {
                return value.len() as isize;
            }
        };
    }
    len_if_slice_storage!(std::string::String);
    len_if_slice_storage!(bool);
    len_if_slice_storage!(isize);
    len_if_slice_storage!(i8);
    len_if_slice_storage!(i16);
    len_if_slice_storage!(i32);
    len_if_slice_storage!(i64);
    len_if_slice_storage!(usize);
    len_if_slice_storage!(u8);
    len_if_slice_storage!(u16);
    len_if_slice_storage!(u32);
    len_if_slice_storage!(u64);
    len_if_slice_storage!(f32);
    len_if_slice_storage!(f64);
    panic_value("reflect: Len of non-slice value");
}

pub fn reflect_value_swapper(value: &dyn Any) -> GorsReflectSwapper {
    let value = erased_any_payload(value);
    let Some(value) = value.downcast_ref::<GorsReflectValue>() else {
        panic_value("reflect: Swapper of non-slice value");
    };
    let value = value.clone();
    Arc::new(Mutex::new(Some(Arc::new(move |i, j| {
        value.swap(i, j);
    }))))
}

pub fn reflect_type_comparable(value: &dyn Any) -> bool {
    if comparable_any(value).is_some() || local_comparable_any(value).is_some() {
        return true;
    }
    if clone_only_any(value).is_some() || local_clone_only_any(value).is_some() {
        return false;
    }
    if let Some(value) = value.downcast_ref::<Box<dyn Any>>() {
        return reflect_type_comparable(value.as_ref());
    }
    if let Some(value) = value.downcast_ref::<Box<dyn Any + Send>>() {
        return reflect_type_comparable(value.as_ref());
    }
    if let Some(value) = value.downcast_ref::<Box<dyn Any + Send + Sync>>() {
        return reflect_type_comparable(value.as_ref());
    }
    if let Some(error) = value.downcast_ref::<Box<dyn error>>() {
        return error.__gors_interface_key().is_comparable();
    }
    if interface_is_nil(value) {
        return false;
    }
    if value.is::<GorsReflectValue>()
        || value.is::<Vec<u8>>()
        || value.is::<Vec<std::string::String>>()
        || value.is::<Vec<Box<dyn Any>>>()
        || value.is::<GorsSliceStorage<u8>>()
        || value.is::<GorsSliceStorage<std::string::String>>()
        || value.is::<GorsSliceStorage<Box<dyn Any>>>()
    {
        return false;
    }
    true
}

pub fn any_eq(left: &dyn Any, right: &dyn Any) -> bool {
    if interface_is_nil(left) || interface_is_nil(right) {
        return interface_is_nil(left) && interface_is_nil(right);
    }
    if let Some(left) = comparable_any(left) {
        return left.eq_any(right);
    }
    if let Some(right) = comparable_any(right) {
        return right.eq_any(left);
    }
    if let Some(left) = local_comparable_any(left) {
        return left.eq_any(right);
    }
    if let Some(right) = local_comparable_any(right) {
        return right.eq_any(left);
    }

    if let (Some(left), Some(right)) = (
        left.downcast_ref::<Box<dyn error>>(),
        right.downcast_ref::<Box<dyn error>>(),
    ) {
        return left.__gors_interface_key() == right.__gors_interface_key();
    }

    let left_type = any_dynamic_type_id(left);
    let right_type = any_dynamic_type_id(right);
    if left_type != right_type {
        return false;
    }
    if left_type.is_some() && (!reflect_type_comparable(left) || !reflect_type_comparable(right)) {
        panic_value("comparing uncomparable dynamic interface value");
    }

    macro_rules! eq_if {
        ($ty:ty) => {
            if let (Some(left), Some(right)) =
                (left.downcast_ref::<$ty>(), right.downcast_ref::<$ty>())
            {
                return left == right;
            }
        };
    }

    eq_if!(std::string::String);
    eq_if!(&'static str);
    eq_if!(bool);
    eq_if!(isize);
    eq_if!(i8);
    eq_if!(i16);
    eq_if!(i32);
    eq_if!(i64);
    eq_if!(usize);
    eq_if!(u8);
    eq_if!(u16);
    eq_if!(u32);
    eq_if!(u64);
    eq_if!(f32);
    eq_if!(f64);
    false
}

pub const r#true: r#bool = true;
pub const r#false: r#bool = false;
pub const iota: int = 0;
pub const nil: Option<()> = None;

#[derive(Debug, Clone, Copy, Default)]
pub struct GorsNilPointer;

impl std::fmt::Display for GorsNilPointer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("nil pointer dereference")
    }
}

impl std::error::Error for GorsNilPointer {}

pub struct GorsPtr<T> {
    inner: Option<GorsPtrInner<T>>,
}

enum GorsPtrInner<T> {
    Direct(Arc<Mutex<T>>),
    Projected(Arc<dyn ProjectedCell<T> + Send + Sync>),
}

impl<T> Clone for GorsPtrInner<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Direct(inner) => Self::Direct(inner.clone()),
            Self::Projected(cell) => Self::Projected(cell.clone()),
        }
    }
}

trait ProjectedCell<T>: Send + Sync {
    fn lock_projected(&self) -> Box<dyn ProjectedGuard<T> + '_>;
    fn cell_ptr(&self) -> *const ();
    fn owner_ptr(&self) -> *const ();
    fn field_key(&self) -> usize;
}

pub trait ProjectedGuard<T>: std::ops::DerefMut<Target = T> {}

impl<T, U> ProjectedGuard<T> for U where U: std::ops::DerefMut<Target = T> {}

struct ProjectedFieldCell<Owner, T, F> {
    owner: GorsPtr<Owner>,
    field_key: usize,
    field: F,
    _field_ty: std::marker::PhantomData<fn() -> T>,
}

struct ProjectedIndexCell<Owner, Container, T, F> {
    owner: GorsPtr<Owner>,
    field_key: usize,
    index: usize,
    field: F,
    _container_ty: std::marker::PhantomData<fn() -> Container>,
    _field_ty: std::marker::PhantomData<fn() -> T>,
}

impl<Owner, T, F> ProjectedCell<T> for ProjectedFieldCell<Owner, T, F>
where
    Owner: Send + 'static,
    T: Default + 'static,
    F: for<'a> Fn(&'a mut Owner) -> &'a mut T + Send + Sync + 'static,
{
    fn lock_projected(&self) -> Box<dyn ProjectedGuard<T> + '_> {
        let value = {
            let mut owner_guard = lock_projected_owner(&self.owner);
            std::mem::take((self.field)(&mut *owner_guard))
        };
        Box::new(ProjectedFieldGuard {
            owner: self.owner.clone(),
            field: &self.field,
            value,
        })
    }

    fn owner_ptr(&self) -> *const () {
        self.owner.ptr_id()
    }

    fn cell_ptr(&self) -> *const () {
        (self as *const Self).cast()
    }

    fn field_key(&self) -> usize {
        self.field_key
    }
}

impl<Owner, Container, T, F> ProjectedCell<T> for ProjectedIndexCell<Owner, Container, T, F>
where
    Owner: Send + 'static,
    Container: std::ops::IndexMut<usize, Output = T> + 'static,
    T: Default + 'static,
    F: for<'a> Fn(&'a mut Owner) -> &'a mut Container + Send + Sync + 'static,
{
    fn lock_projected(&self) -> Box<dyn ProjectedGuard<T> + '_> {
        let value = {
            let mut owner_guard = lock_projected_owner(&self.owner);
            std::mem::take(&mut (self.field)(&mut *owner_guard)[self.index])
        };
        Box::new(ProjectedIndexGuard {
            owner: self.owner.clone(),
            field: &self.field,
            index: self.index,
            value,
            _container_ty: std::marker::PhantomData,
        })
    }

    fn owner_ptr(&self) -> *const () {
        self.owner.ptr_id()
    }

    fn cell_ptr(&self) -> *const () {
        (self as *const Self).cast()
    }

    fn field_key(&self) -> usize {
        self.field_key
            .wrapping_mul(1_000_003)
            .wrapping_add(self.index)
    }
}

struct IdentityProjectedFieldCell<Owner, T> {
    owner: GorsPtr<Owner>,
    field_key: usize,
    _field_ty: std::marker::PhantomData<fn() -> T>,
}

impl<Owner, T> ProjectedCell<T> for IdentityProjectedFieldCell<Owner, T>
where
    Owner: Send + 'static,
    T: 'static,
{
    fn lock_projected(&self) -> Box<dyn ProjectedGuard<T> + '_> {
        Box::new(UnsupportedProjectedGuard {
            _field_ty: std::marker::PhantomData,
        })
    }

    fn owner_ptr(&self) -> *const () {
        self.owner.ptr_id()
    }

    fn cell_ptr(&self) -> *const () {
        (self as *const Self).cast()
    }

    fn field_key(&self) -> usize {
        self.field_key
    }
}

struct ProjectedFieldGuard<'a, Owner, T: Default, F>
where
    F: for<'b> Fn(&'b mut Owner) -> &'b mut T,
{
    owner: GorsPtr<Owner>,
    field: &'a F,
    value: T,
}

struct ProjectedIndexGuard<'a, Owner, Container, T: Default, F>
where
    Container: std::ops::IndexMut<usize, Output = T>,
    F: for<'b> Fn(&'b mut Owner) -> &'b mut Container,
{
    owner: GorsPtr<Owner>,
    field: &'a F,
    index: usize,
    value: T,
    _container_ty: std::marker::PhantomData<fn() -> Container>,
}

impl<Owner, T, F> std::ops::Deref for ProjectedFieldGuard<'_, Owner, T, F>
where
    T: Default,
    F: for<'a> Fn(&'a mut Owner) -> &'a mut T,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<Owner, Container, T, F> std::ops::Deref for ProjectedIndexGuard<'_, Owner, Container, T, F>
where
    Container: std::ops::IndexMut<usize, Output = T>,
    T: Default,
    F: for<'a> Fn(&'a mut Owner) -> &'a mut Container,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<Owner, T, F> std::ops::DerefMut for ProjectedFieldGuard<'_, Owner, T, F>
where
    T: Default,
    F: for<'a> Fn(&'a mut Owner) -> &'a mut T,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<Owner, Container, T, F> std::ops::DerefMut for ProjectedIndexGuard<'_, Owner, Container, T, F>
where
    Container: std::ops::IndexMut<usize, Output = T>,
    T: Default,
    F: for<'a> Fn(&'a mut Owner) -> &'a mut Container,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<Owner, T, F> Drop for ProjectedFieldGuard<'_, Owner, T, F>
where
    T: Default,
    F: for<'a> Fn(&'a mut Owner) -> &'a mut T,
{
    fn drop(&mut self) {
        let mut owner = lock_projected_owner(&self.owner);
        *(self.field)(&mut *owner) = std::mem::take(&mut self.value);
    }
}

impl<Owner, Container, T, F> Drop for ProjectedIndexGuard<'_, Owner, Container, T, F>
where
    Container: std::ops::IndexMut<usize, Output = T>,
    T: Default,
    F: for<'a> Fn(&'a mut Owner) -> &'a mut Container,
{
    fn drop(&mut self) {
        let mut owner = lock_projected_owner(&self.owner);
        (self.field)(&mut *owner)[self.index] = std::mem::take(&mut self.value);
    }
}

struct UnsupportedProjectedGuard<T> {
    _field_ty: std::marker::PhantomData<fn() -> T>,
}

impl<T> std::ops::Deref for UnsupportedProjectedGuard<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        panic_value("projected non-clone field cannot be locked")
    }
}

impl<T> std::ops::DerefMut for UnsupportedProjectedGuard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        panic_value("projected non-clone field cannot be locked")
    }
}

fn lock_projected_owner<T>(owner: &GorsPtr<T>) -> GorsPtrGuard<'_, T> {
    match owner.lock() {
        Ok(guard) => guard,
        Err(err) => panic_value(err),
    }
}

pub enum GorsPtrGuard<'a, T> {
    Direct(MutexGuard<'a, T>),
    Projected(Box<dyn ProjectedGuard<T> + 'a>),
}

impl<T> std::ops::Deref for GorsPtrGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Direct(guard) => guard,
            Self::Projected(guard) => std::ops::Deref::deref(&**guard),
        }
    }
}

impl<T> std::ops::DerefMut for GorsPtrGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Direct(guard) => guard,
            Self::Projected(guard) => std::ops::DerefMut::deref_mut(&mut **guard),
        }
    }
}

impl<T> Clone for GorsPtr<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Default for GorsPtr<T> {
    fn default() -> Self {
        Self::nil()
    }
}

impl<T> GorsPtr<T> {
    pub fn nil() -> Self {
        Self { inner: None }
    }

    pub fn new(value: T) -> Self {
        Self {
            inner: Some(GorsPtrInner::Direct(Arc::new(Mutex::new(value)))),
        }
    }

    pub fn from_arc(inner: Arc<Mutex<T>>) -> Self {
        Self {
            inner: Some(GorsPtrInner::Direct(inner)),
        }
    }

    pub fn from_arc_field<Owner, F>(owner: Arc<Mutex<Owner>>, field_key: usize, field: F) -> Self
    where
        Owner: Send + 'static,
        T: Default + 'static,
        F: for<'a> Fn(&'a mut Owner) -> &'a mut T + Send + Sync + 'static,
    {
        Self::from_ptr_field(GorsPtr::from_arc(owner), field_key, field)
    }

    pub fn from_ptr_field<Owner, F>(owner: GorsPtr<Owner>, field_key: usize, field: F) -> Self
    where
        Owner: Send + 'static,
        T: Default + 'static,
        F: for<'a> Fn(&'a mut Owner) -> &'a mut T + Send + Sync + 'static,
    {
        Self {
            inner: Some(GorsPtrInner::Projected(Arc::new(ProjectedFieldCell {
                owner,
                field_key,
                field,
                _field_ty: std::marker::PhantomData,
            }))),
        }
    }

    pub fn from_ptr_index<Owner, Container, F>(
        owner: GorsPtr<Owner>,
        field_key: usize,
        index: usize,
        field: F,
    ) -> Self
    where
        Owner: Send + 'static,
        Container: std::ops::IndexMut<usize, Output = T> + 'static,
        T: Default + 'static,
        F: for<'a> Fn(&'a mut Owner) -> &'a mut Container + Send + Sync + 'static,
    {
        Self {
            inner: Some(GorsPtrInner::Projected(Arc::new(ProjectedIndexCell {
                owner,
                field_key,
                index,
                field,
                _container_ty: std::marker::PhantomData,
                _field_ty: std::marker::PhantomData,
            }))),
        }
    }

    pub fn from_ptr_field_identity<Owner, F>(
        owner: GorsPtr<Owner>,
        field_key: usize,
        _field: F,
    ) -> Self
    where
        Owner: Send + 'static,
        T: 'static,
        F: for<'a> Fn(&'a mut Owner) -> &'a mut T + 'static,
    {
        Self {
            inner: Some(GorsPtrInner::Projected(Arc::new(
                IdentityProjectedFieldCell {
                    owner,
                    field_key,
                    _field_ty: std::marker::PhantomData,
                },
            ))),
        }
    }

    pub fn is_nil(&self) -> bool {
        self.inner.is_none()
    }

    pub fn lock(&self) -> Result<GorsPtrGuard<'_, T>, GorsNilPointer> {
        let inner = self.inner.as_ref().ok_or(GorsNilPointer)?;
        match inner {
            GorsPtrInner::Direct(inner) => Ok(GorsPtrGuard::Direct(
                inner
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            )),
            GorsPtrInner::Projected(cell) => Ok(GorsPtrGuard::Projected(cell.lock_projected())),
        }
    }

    pub fn ptr_eq(left: &Self, right: &Self) -> bool {
        match (&left.inner, &right.inner) {
            (None, None) => true,
            (Some(GorsPtrInner::Direct(left)), Some(GorsPtrInner::Direct(right))) => {
                Arc::ptr_eq(left, right)
            }
            (Some(GorsPtrInner::Projected(left)), Some(GorsPtrInner::Projected(right))) => {
                left.owner_ptr() == right.owner_ptr() && left.field_key() == right.field_key()
            }
            _ => false,
        }
    }

    pub fn interface_key(&self) -> GorsInterfaceKey {
        match &self.inner {
            None => GorsInterfaceKey::for_ptr::<T>(std::ptr::null()),
            Some(GorsPtrInner::Direct(inner)) => {
                GorsInterfaceKey::for_ptr::<T>(Arc::as_ptr(inner).cast::<()>())
            }
            Some(GorsPtrInner::Projected(cell)) => {
                GorsInterfaceKey::for_projected_ptr::<T>(cell.owner_ptr(), cell.field_key())
            }
        }
    }

    pub fn ptr_id(&self) -> *const () {
        match &self.inner {
            None => std::ptr::null(),
            Some(GorsPtrInner::Direct(inner)) => Arc::as_ptr(inner).cast(),
            Some(GorsPtrInner::Projected(cell)) => cell.cell_ptr(),
        }
    }
}

impl<T> PartialEq for GorsPtr<T> {
    fn eq(&self, other: &Self) -> bool {
        Self::ptr_eq(self, other)
    }
}

impl<T> Eq for GorsPtr<T> {}

struct GorsSliceAliasResliceErased {
    values: Box<dyn Any>,
    start: usize,
    result_capacity: usize,
    detached: bool,
}

thread_local! {
    static GORS_SLICE_ALIAS_RESLICE_STACKS: RefCell<HashMap<usize, Vec<Vec<GorsSliceAliasResliceErased>>>> = RefCell::new(HashMap::new());
}

pub struct GorsSliceAliasReslice<T> {
    pub values: Vec<T>,
    pub start: usize,
    pub result_capacity: usize,
    pub detached: bool,
}

pub struct GorsSliceAliasTransaction {
    pointer_id: usize,
    active: bool,
}

pub fn begin_gors_slice_alias_transaction(pointer_id: *const ()) -> GorsSliceAliasTransaction {
    let pointer_id = pointer_id as usize;
    GORS_SLICE_ALIAS_RESLICE_STACKS.with(|stacks| {
        stacks
            .borrow_mut()
            .entry(pointer_id)
            .or_default()
            .push(Vec::new());
    });
    GorsSliceAliasTransaction {
        pointer_id,
        active: true,
    }
}

pub fn record_gors_slice_alias_reslice<T: Clone + 'static>(
    pointer_id: *const (),
    values: &[T],
    start: usize,
    result_capacity: usize,
) {
    GORS_SLICE_ALIAS_RESLICE_STACKS.with(|stacks| {
        let mut stacks = stacks.borrow_mut();
        let Some(events) = stacks
            .get_mut(&(pointer_id as usize))
            .and_then(|stack| stack.last_mut())
        else {
            return;
        };
        events.push(GorsSliceAliasResliceErased {
            values: Box::new(values.to_vec()),
            start,
            result_capacity,
            detached: false,
        });
    });
}

pub fn record_gors_slice_alias_detach<T: Clone + 'static>(pointer_id: *const (), values: &[T]) {
    GORS_SLICE_ALIAS_RESLICE_STACKS.with(|stacks| {
        let mut stacks = stacks.borrow_mut();
        let Some(events) = stacks
            .get_mut(&(pointer_id as usize))
            .and_then(|stack| stack.last_mut())
        else {
            return;
        };
        events.push(GorsSliceAliasResliceErased {
            values: Box::new(values.to_vec()),
            start: 0,
            result_capacity: 0,
            detached: true,
        });
    });
}

impl GorsSliceAliasTransaction {
    fn take_events(&mut self) -> Vec<GorsSliceAliasResliceErased> {
        if !self.active {
            return Vec::new();
        }
        self.active = false;
        GORS_SLICE_ALIAS_RESLICE_STACKS.with(|stacks| {
            let mut stacks = stacks.borrow_mut();
            let Some(stack) = stacks.get_mut(&self.pointer_id) else {
                return Vec::new();
            };
            let events = stack.pop().unwrap_or_default();
            if stack.is_empty() {
                stacks.remove(&self.pointer_id);
            }
            events
        })
    }

    pub fn finish<T: 'static>(mut self) -> Vec<GorsSliceAliasReslice<T>> {
        self.take_events()
            .into_iter()
            .map(|event| {
                let values = match event.values.downcast::<Vec<T>>() {
                    Ok(values) => *values,
                    Err(_) => panic_value("slice alias transaction type mismatch"),
                };
                GorsSliceAliasReslice {
                    values,
                    start: event.start,
                    result_capacity: event.result_capacity,
                    detached: event.detached,
                }
            })
            .collect()
    }
}

impl Drop for GorsSliceAliasTransaction {
    fn drop(&mut self) {
        let _ = self.take_events();
    }
}

thread_local! {
    static RECOVER_PAYLOAD: RefCell<Option<Box<dyn Any + Send>>> = const { RefCell::new(None) };
    static SUPPRESS_GO_PANIC_HOOK_DEPTH: Cell<usize> = const { Cell::new(0) };
}

static INSTALL_GO_PANIC_HOOK: Once = Once::new();

struct GoPanicHookGuard;

impl GoPanicHookGuard {
    fn suppress() -> Self {
        INSTALL_GO_PANIC_HOOK.call_once(|| {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                let suppressed = SUPPRESS_GO_PANIC_HOOK_DEPTH.with(|depth| depth.get() != 0);
                if !suppressed {
                    previous(info);
                }
            }));
        });
        SUPPRESS_GO_PANIC_HOOK_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
        Self
    }
}

impl Drop for GoPanicHookGuard {
    fn drop(&mut self) {
        SUPPRESS_GO_PANIC_HOOK_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

struct UnrecoveredGoPanic(Box<dyn Any + Send>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum __GorsReflectKind {
    Invalid,
    Bool,
    Int,
    Int8,
    Int16,
    Int32,
    Int64,
    Uint,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Uintptr,
    Float32,
    Float64,
    Complex64,
    Complex128,
    Array,
    Chan,
    Func,
    Interface,
    Map,
    Pointer,
    Slice,
    String,
    Struct,
    UnsafePointer,
}

pub trait __GorsReflectKindValue {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind;
}

impl<T: __GorsReflectKindValue + ?Sized> __GorsReflectKindValue for &T {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        (**self).__gors_reflect_kind()
    }
}

impl __GorsReflectKindValue for Box<dyn Any> {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        reflect_kind_of_any(&**self)
    }
}

impl __GorsReflectKindValue for r#bool {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Bool
    }
}

impl __GorsReflectKindValue for int {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Int
    }
}

impl __GorsReflectKindValue for int8 {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Int8
    }
}

impl __GorsReflectKindValue for int16 {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Int16
    }
}

impl __GorsReflectKindValue for int32 {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Int32
    }
}

impl __GorsReflectKindValue for int64 {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Int64
    }
}

impl __GorsReflectKindValue for uint {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Uint
    }
}

impl __GorsReflectKindValue for uint8 {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Uint8
    }
}

impl __GorsReflectKindValue for uint16 {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Uint16
    }
}

impl __GorsReflectKindValue for uint32 {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Uint32
    }
}

impl __GorsReflectKindValue for uint64 {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Uint64
    }
}

impl __GorsReflectKindValue for float32 {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Float32
    }
}

impl __GorsReflectKindValue for float64 {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Float64
    }
}

impl __GorsReflectKindValue for complex64 {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Complex64
    }
}

impl __GorsReflectKindValue for complex128 {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Complex128
    }
}

impl __GorsReflectKindValue for std::string::String {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::String
    }
}

impl __GorsReflectKindValue for str {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::String
    }
}

impl<T> __GorsReflectKindValue for Vec<T> {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Slice
    }
}

impl<T> __GorsReflectKindValue for GorsSliceStorage<T> {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Slice
    }
}

impl<T, const N: usize> __GorsReflectKindValue for [T; N] {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Array
    }
}

impl<K, V> __GorsReflectKindValue for HashMap<K, V> {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Map
    }
}

impl<K, V> __GorsReflectKindValue for GorsMap<K, V> {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Map
    }
}

impl<T> __GorsReflectKindValue for Chan<T> {
    fn __gors_reflect_kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Chan
    }
}

#[inline]
pub fn reflect_kind_is<T: __GorsReflectKindValue + ?Sized>(
    value: &T,
    kind: __GorsReflectKind,
) -> bool {
    value.__gors_reflect_kind() == kind
}

fn reflect_kind_of_any(value: &dyn Any) -> __GorsReflectKind {
    let value = erased_any_payload(value);
    if value.is::<r#bool>() {
        __GorsReflectKind::Bool
    } else if value.is::<int>() {
        __GorsReflectKind::Int
    } else if value.is::<int8>() {
        __GorsReflectKind::Int8
    } else if value.is::<int16>() {
        __GorsReflectKind::Int16
    } else if value.is::<int32>() {
        __GorsReflectKind::Int32
    } else if value.is::<int64>() {
        __GorsReflectKind::Int64
    } else if value.is::<uint>() {
        __GorsReflectKind::Uint
    } else if value.is::<uint8>() {
        __GorsReflectKind::Uint8
    } else if value.is::<uint16>() {
        __GorsReflectKind::Uint16
    } else if value.is::<uint32>() {
        __GorsReflectKind::Uint32
    } else if value.is::<uint64>() {
        __GorsReflectKind::Uint64
    } else if value.is::<float32>() {
        __GorsReflectKind::Float32
    } else if value.is::<float64>() {
        __GorsReflectKind::Float64
    } else if value.is::<complex64>() {
        __GorsReflectKind::Complex64
    } else if value.is::<complex128>() {
        __GorsReflectKind::Complex128
    } else if value.is::<std::string::String>() || value.is::<&str>() {
        __GorsReflectKind::String
    } else {
        __GorsReflectKind::Invalid
    }
}

pub trait Len {
    fn len_value(&self) -> usize;
}

pub fn lock_func<T: ?Sized>(func: &Arc<Mutex<T>>) -> MutexGuard<'_, T> {
    func.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub trait ByteSeq {
    fn byte_at(&self, index: usize) -> u8;
    fn byte_slice(&self, start: usize, end: usize) -> Vec<u8>;
}

fn byte_at_or_panic(bytes: &[u8], index: usize) -> u8 {
    bytes
        .get(index)
        .copied()
        .unwrap_or_else(|| panic_value("index out of range"))
}

fn byte_slice_or_panic(bytes: &[u8], start: usize, end: usize) -> Vec<u8> {
    bytes
        .get(start..end)
        .map(<[u8]>::to_vec)
        .unwrap_or_else(|| panic_value("slice bounds out of range"))
}

const GO_STRING_ESCAPE: char = '\u{10ffff}';
const GO_STRING_ESCAPED_BYTE_BASE: u32 = 0xe000;

fn push_go_string_valid_segment(out: &mut std::string::String, value: &str) {
    for ch in value.chars() {
        out.push(ch);
        if ch == GO_STRING_ESCAPE {
            out.push(GO_STRING_ESCAPE);
        }
    }
}

/// Encode arbitrary Go string bytes in the generated Rust `String` ABI.
///
/// Valid UTF-8 stays readable. Invalid bytes use an escaped private-use scalar,
/// and the escape scalar itself is doubled, so decoding is lossless for every
/// possible byte sequence.
pub fn go_string_from_bytes(bytes: &[u8]) -> std::string::String {
    let mut out = std::string::String::with_capacity(bytes.len());
    let mut remaining = bytes;
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                push_go_string_valid_segment(&mut out, valid);
                break;
            }
            Err(error) => {
                let valid_len = error.valid_up_to();
                if let Some(valid) = remaining
                    .get(..valid_len)
                    .and_then(|prefix| std::str::from_utf8(prefix).ok())
                    .filter(|_| valid_len != 0)
                {
                    push_go_string_valid_segment(&mut out, valid);
                }
                let invalid_len = error
                    .error_len()
                    .unwrap_or_else(|| remaining.len().saturating_sub(valid_len))
                    .max(1);
                let invalid_end = valid_len.saturating_add(invalid_len).min(remaining.len());
                let Some(invalid_bytes) = remaining.get(valid_len..invalid_end) else {
                    break;
                };
                for byte in invalid_bytes {
                    out.push(GO_STRING_ESCAPE);
                    if let Some(escaped) =
                        char::from_u32(GO_STRING_ESCAPED_BYTE_BASE + u32::from(*byte))
                    {
                        out.push(escaped);
                    }
                }
                if invalid_end == 0 {
                    break;
                }
                let Some(next) = remaining.get(invalid_end..) else {
                    break;
                };
                remaining = next;
            }
        }
    }
    out
}

pub fn go_string_bytes(value: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != GO_STRING_ESCAPE {
            let mut encoded = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
            continue;
        }
        match chars.next() {
            Some(next) if next == GO_STRING_ESCAPE => {
                let mut encoded = [0u8; 4];
                out.extend_from_slice(next.encode_utf8(&mut encoded).as_bytes());
            }
            Some(next)
                if (GO_STRING_ESCAPED_BYTE_BASE
                    ..=GO_STRING_ESCAPED_BYTE_BASE + u32::from(u8::MAX))
                    .contains(&(next as u32)) =>
            {
                out.push((next as u32 - GO_STRING_ESCAPED_BYTE_BASE) as u8);
            }
            Some(next) => {
                let mut encoded = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
                out.extend_from_slice(next.encode_utf8(&mut encoded).as_bytes());
            }
            None => {
                let mut encoded = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
            }
        }
    }
    out
}

fn decode_go_utf8_rune(bytes: &[u8]) -> (i32, usize) {
    let Some(&first) = bytes.first() else {
        return (0xfffd, 0);
    };
    if first < 0x80 {
        return (i32::from(first), 1);
    }
    let (width, minimum, mut value) = match first {
        0xc2..=0xdf => (2usize, 0x80u32, u32::from(first & 0x1f)),
        0xe0..=0xef => (3usize, 0x800u32, u32::from(first & 0x0f)),
        0xf0..=0xf4 => (4usize, 0x10000u32, u32::from(first & 0x07)),
        _ => return (0xfffd, 1),
    };
    let Some(sequence) = bytes.get(..width) else {
        return (0xfffd, 1);
    };
    let Some(continuations) = sequence.get(1..) else {
        return (0xfffd, 1);
    };
    for &next in continuations {
        if next & 0xc0 != 0x80 {
            return (0xfffd, 1);
        }
        value = (value << 6) | u32::from(next & 0x3f);
    }
    let Some(&second) = sequence.get(1) else {
        return (0xfffd, 1);
    };
    if value < minimum
        || value > char::MAX as u32
        || (0xd800..=0xdfff).contains(&value)
        || (first == 0xe0 && second < 0xa0)
        || (first == 0xed && second >= 0xa0)
        || (first == 0xf0 && second < 0x90)
        || (first == 0xf4 && second >= 0x90)
    {
        return (0xfffd, 1);
    }
    (value as i32, width)
}

pub fn go_string_char_indices(value: &str) -> std::vec::IntoIter<(isize, i32)> {
    let bytes = go_string_bytes(value);
    let mut values = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        let Some(remaining) = bytes.get(index..) else {
            break;
        };
        let (rune, width) = decode_go_utf8_rune(remaining);
        values.push((index as isize, rune));
        index = index.saturating_add(width.max(1));
    }
    values.into_iter()
}

pub fn go_string_runes(value: &str) -> Vec<i32> {
    go_string_char_indices(value)
        .map(|(_, rune)| rune)
        .collect()
}

pub fn go_string_slice(value: &str, start: usize, end: usize) -> std::string::String {
    let bytes = go_string_bytes(value);
    let Some(slice) = bytes.get(start..end) else {
        panic_value("slice bounds out of range");
    };
    go_string_from_bytes(slice)
}

impl ByteSeq for std::string::String {
    fn byte_at(&self, index: usize) -> u8 {
        byte_at_or_panic(&go_string_bytes(self), index)
    }

    fn byte_slice(&self, start: usize, end: usize) -> Vec<u8> {
        byte_slice_or_panic(&go_string_bytes(self), start, end)
    }
}

impl ByteSeq for str {
    fn byte_at(&self, index: usize) -> u8 {
        byte_at_or_panic(&go_string_bytes(self), index)
    }

    fn byte_slice(&self, start: usize, end: usize) -> Vec<u8> {
        byte_slice_or_panic(&go_string_bytes(self), start, end)
    }
}

impl ByteSeq for Vec<u8> {
    fn byte_at(&self, index: usize) -> u8 {
        byte_at_or_panic(self, index)
    }

    fn byte_slice(&self, start: usize, end: usize) -> Vec<u8> {
        byte_slice_or_panic(self, start, end)
    }
}

impl ByteSeq for [u8] {
    fn byte_at(&self, index: usize) -> u8 {
        byte_at_or_panic(self, index)
    }

    fn byte_slice(&self, start: usize, end: usize) -> Vec<u8> {
        byte_slice_or_panic(self, start, end)
    }
}

impl<const N: usize> ByteSeq for [u8; N] {
    fn byte_at(&self, index: usize) -> u8 {
        self.as_slice().byte_at(index)
    }

    fn byte_slice(&self, start: usize, end: usize) -> Vec<u8> {
        self.as_slice().byte_slice(start, end)
    }
}

impl<T: ByteSeq + ?Sized> ByteSeq for &T {
    fn byte_at(&self, index: usize) -> u8 {
        (**self).byte_at(index)
    }

    fn byte_slice(&self, start: usize, end: usize) -> Vec<u8> {
        (**self).byte_slice(start, end)
    }
}

impl<T: ByteSeq + ?Sized> ByteSeq for &mut T {
    fn byte_at(&self, index: usize) -> u8 {
        (**self).byte_at(index)
    }

    fn byte_slice(&self, start: usize, end: usize) -> Vec<u8> {
        (**self).byte_slice(start, end)
    }
}

#[inline]
pub fn byte_at<T: ByteSeq + ?Sized>(value: &T, index: usize) -> u8 {
    value.byte_at(index)
}

#[inline]
pub fn byte_slice<T: ByteSeq + ?Sized>(value: &T, start: usize, end: usize) -> Vec<u8> {
    value.byte_slice(start, end)
}

#[inline]
pub fn string_from_byte_seq<T: ByteSeq + Len + ?Sized>(value: &T) -> std::string::String {
    go_string_from_bytes(&value.byte_slice(0, value.len_value()))
}

impl<T> Len for Vec<T> {
    fn len_value(&self) -> usize {
        self.len()
    }
}

impl Len for std::string::String {
    fn len_value(&self) -> usize {
        go_string_bytes(self).len()
    }
}

impl Len for str {
    fn len_value(&self) -> usize {
        go_string_bytes(self).len()
    }
}

impl<T> Len for [T] {
    fn len_value(&self) -> usize {
        self.len()
    }
}

impl<T, const N: usize> Len for [T; N] {
    fn len_value(&self) -> usize {
        N
    }
}

impl<T, const N: usize> Len for GorsPtr<[T; N]> {
    fn len_value(&self) -> usize {
        N
    }
}

impl<K, V> Len for HashMap<K, V> {
    fn len_value(&self) -> usize {
        self.len()
    }
}

impl<K, V> Len for GorsMap<K, V> {
    fn len_value(&self) -> usize {
        self.len()
    }
}

impl<T> Len for Chan<T> {
    fn len_value(&self) -> usize {
        self.len()
    }
}

impl<T: Len + ?Sized> Len for &T {
    fn len_value(&self) -> usize {
        (**self).len_value()
    }
}

impl<T: Len + ?Sized> Len for &mut T {
    fn len_value(&self) -> usize {
        (**self).len_value()
    }
}

impl<T: Len> Len for std::sync::LazyLock<T> {
    fn len_value(&self) -> usize {
        (**self).len_value()
    }
}

#[inline]
pub fn len<T: Len + ?Sized>(v: &T) -> usize {
    v.len_value()
}

pub trait Cap {
    fn cap_value(&self) -> usize;
}

impl<T> Cap for Vec<T> {
    fn cap_value(&self) -> usize {
        self.capacity()
    }
}

impl<T> Cap for [T] {
    fn cap_value(&self) -> usize {
        // A borrowed Rust slice does not retain the Go slice header's backing
        // capacity, so its visible length is the only representable capacity.
        self.len()
    }
}

impl<T, const N: usize> Cap for [T; N] {
    fn cap_value(&self) -> usize {
        N
    }
}

impl<T, const N: usize> Cap for GorsPtr<[T; N]> {
    fn cap_value(&self) -> usize {
        N
    }
}

impl<T> Cap for Chan<T> {
    fn cap_value(&self) -> usize {
        self.cap()
    }
}

impl<T: Cap + ?Sized> Cap for &T {
    fn cap_value(&self) -> usize {
        (**self).cap_value()
    }
}

impl<T: Cap + ?Sized> Cap for &mut T {
    fn cap_value(&self) -> usize {
        (**self).cap_value()
    }
}

impl<T: Cap> Cap for std::sync::LazyLock<T> {
    fn cap_value(&self) -> usize {
        (**self).cap_value()
    }
}

#[inline]
pub fn cap<T: Cap + ?Sized>(v: &T) -> usize {
    v.cap_value()
}

#[inline]
pub fn go_slice<T: Clone + Default>(
    source: &[T],
    source_capacity: usize,
    start: usize,
    end: usize,
    max: usize,
) -> Vec<T> {
    if start > end || end > max || max > source_capacity {
        panic_value("slice bounds out of range");
    }

    let mut result = Vec::with_capacity(max - start);
    let initialized_end = end.min(source.len());
    if start < initialized_end {
        let Some(initialized) = source.get(start..initialized_end) else {
            panic_value("slice bounds out of range");
        };
        result.extend_from_slice(initialized);
    }
    result.resize_with(end - start, Default::default);
    result
}

/// Owned backing storage for a Go slice header whose capacity can be exposed.
///
/// Rust's `Vec<T>` cannot safely retain initialized values beyond `len()`: if
/// its length is shortened those values are dropped, and changing the length
/// with `set_len` would make later reallocation and destruction unsound. This
/// representation instead keeps the entire Go capacity initialized in
/// `backing` while tracking the visible header separately. Reslicing therefore
/// changes only `start`, `len`, and `capacity`; writes made through the capacity
/// range remain alive until a later header makes them visible.
pub struct GorsSliceStorage<T> {
    backing: Vec<T>,
    start: usize,
    len: usize,
    capacity: usize,
}

impl<T> GorsSliceStorage<T> {
    /// Build storage from a fully initialized backing array.
    ///
    /// The backing vector's current length, rather than its allocation
    /// capacity, is the Go capacity because only initialized elements may be
    /// exposed safely.
    pub fn from_initialized_backing(backing: Vec<T>, visible_len: usize) -> Self {
        let capacity = backing.len();
        if visible_len > capacity {
            panic_value("slice bounds out of range");
        }
        Self {
            backing,
            start: 0,
            len: visible_len,
            capacity,
        }
    }

    /// Construct a Go slice header with a caller-provided element zero value.
    ///
    /// Compiler-generated interface and other runtime-owned element types do
    /// not necessarily implement Rust's `Default`, even though the compiler
    /// can still emit their Go zero value explicitly.
    pub fn with_len_capacity_by(len: usize, capacity: usize, zero: impl FnMut() -> T) -> Self {
        if len > capacity {
            panic_value("slice bounds out of range");
        }
        let mut backing = Vec::with_capacity(capacity);
        backing.resize_with(capacity, zero);
        Self {
            backing,
            start: 0,
            len,
            capacity,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    #[inline]
    pub fn visible(&self) -> &[T] {
        self.backing
            .get(self.absolute_range(0, self.len, self.len))
            .unwrap_or_else(|| panic_value("slice bounds out of range"))
    }

    #[inline]
    pub fn visible_mut(&mut self) -> &mut [T] {
        let range = self.absolute_range(0, self.len, self.len);
        self.backing
            .get_mut(range)
            .unwrap_or_else(|| panic_value("slice bounds out of range"))
    }

    /// Return the fully initialized portion of the backing array reachable
    /// through this header's capacity.
    #[inline]
    pub fn full(&self) -> &[T] {
        self.backing
            .get(self.absolute_range(0, self.capacity, self.capacity))
            .unwrap_or_else(|| panic_value("slice bounds out of range"))
    }

    #[inline]
    pub fn full_mut(&mut self) -> &mut [T] {
        let range = self.absolute_range(0, self.capacity, self.capacity);
        self.backing
            .get_mut(range)
            .unwrap_or_else(|| panic_value("slice bounds out of range"))
    }

    /// Checked range within the current visible length.
    #[inline]
    pub fn visible_range(&self, low: usize, high: usize) -> &[T] {
        let range = self.absolute_range(low, high, self.len);
        self.backing
            .get(range)
            .unwrap_or_else(|| panic_value("slice bounds out of range"))
    }

    #[inline]
    pub fn visible_range_mut(&mut self, low: usize, high: usize) -> &mut [T] {
        let range = self.absolute_range(low, high, self.len);
        self.backing
            .get_mut(range)
            .unwrap_or_else(|| panic_value("slice bounds out of range"))
    }

    /// Checked range within the current capacity.
    ///
    /// `max` models the third index of a Go full-slice expression. Omitting it
    /// permits the range to use the header's whole capacity.
    #[inline]
    pub fn full_range(&self, low: usize, high: usize, max: Option<usize>) -> &[T] {
        let limit = self.checked_full_limit(low, high, max);
        let range = self.absolute_range(low, high, limit);
        self.backing
            .get(range)
            .unwrap_or_else(|| panic_value("slice bounds out of range"))
    }

    #[inline]
    pub fn full_range_mut(&mut self, low: usize, high: usize, max: Option<usize>) -> &mut [T] {
        let limit = self.checked_full_limit(low, high, max);
        let range = self.absolute_range(low, high, limit);
        self.backing
            .get_mut(range)
            .unwrap_or_else(|| panic_value("slice bounds out of range"))
    }

    /// Apply a Go slice expression to this owned header.
    pub fn reslice(&mut self, start: usize, end: usize, max: usize) {
        self.check_bounds(start, end, max, self.capacity);
        self.start = self
            .start
            .checked_add(start)
            .unwrap_or_else(|| panic_value("slice bounds out of range"));
        self.len = end - start;
        self.capacity = max - start;
    }

    #[must_use]
    pub fn resliced(mut self, start: usize, end: usize, max: usize) -> Self {
        self.reslice(start, end, max);
        self
    }

    pub fn push_visible(&mut self, value: T) {
        self.extend_visible(std::iter::once(value));
    }

    pub fn extend_visible<I>(&mut self, values: I)
    where
        I: IntoIterator<Item = T>,
    {
        let values: Vec<T> = values.into_iter().collect();
        if values.is_empty() {
            return;
        }

        let required = self
            .len
            .checked_add(values.len())
            .unwrap_or_else(|| panic_value("slice bounds out of range"));
        if required <= self.capacity {
            for value in values {
                let index = self
                    .start
                    .checked_add(self.len)
                    .unwrap_or_else(|| panic_value("slice bounds out of range"));
                let Some(slot) = self.backing.get_mut(index) else {
                    panic_value("slice bounds out of range");
                };
                *slot = value;
                self.len += 1;
            }
            return;
        }

        // A type without a zero-value factory cannot safely expose spare
        // allocation as Go capacity. Move only the visible values, append the
        // new values, and make the fully initialized length the logical cap.
        let old_backing = std::mem::take(&mut self.backing);
        let mut grown = Vec::with_capacity(required);
        grown.extend(old_backing.into_iter().skip(self.start).take(self.len));
        grown.extend(values);
        self.backing = grown;
        self.start = 0;
        self.len = required;
        self.capacity = required;
    }

    /// Consume the storage as an ordinary visible `Vec<T>`.
    ///
    /// Hidden initialized elements are dropped exactly once. The resulting
    /// vector reserves at least the Go header's remaining capacity, but callers
    /// that need hidden-tail values must retain `GorsSliceStorage` instead.
    pub fn into_visible_vec(mut self) -> Vec<T> {
        if self.start == 0 && self.capacity == self.backing.len() {
            self.backing.truncate(self.len);
            return self.backing;
        }

        let end = self
            .start
            .checked_add(self.len)
            .unwrap_or_else(|| panic_value("slice bounds out of range"));
        if end > self.backing.len() {
            panic_value("slice bounds out of range");
        }
        let mut visible = Vec::with_capacity(self.capacity);
        visible.extend(self.backing.drain(self.start..end));
        visible
    }

    fn checked_full_limit(&self, low: usize, high: usize, max: Option<usize>) -> usize {
        let limit = max.unwrap_or(self.capacity);
        self.check_bounds(low, high, limit, self.capacity);
        limit
    }

    fn absolute_range(
        &self,
        low: usize,
        high: usize,
        relative_limit: usize,
    ) -> std::ops::Range<usize> {
        self.check_bounds(low, high, relative_limit, self.capacity);
        let start = self
            .start
            .checked_add(low)
            .unwrap_or_else(|| panic_value("slice bounds out of range"));
        let end = self
            .start
            .checked_add(high)
            .unwrap_or_else(|| panic_value("slice bounds out of range"));
        start..end
    }

    fn check_bounds(&self, low: usize, high: usize, max: usize, capacity: usize) {
        if low > high || high > max || max > capacity {
            panic_value("slice bounds out of range");
        }
    }
}

impl<T: Default> GorsSliceStorage<T> {
    /// Promote an ordinary visible vector into initialized Go slice storage.
    pub fn from_vec(mut visible: Vec<T>) -> Self {
        let len = visible.len();
        let capacity = visible.capacity();
        visible.resize_with(capacity, T::default);
        Self {
            backing: visible,
            start: 0,
            len,
            capacity,
        }
    }

    /// Take an ordinary vector into capacity-preserving storage.
    ///
    /// Paired with [`Self::into_visible_vec`], this gives compiler-generated
    /// transactions a safe way to expose initialized `len..cap` storage and
    /// restore the original visible header without moving or double-dropping
    /// any element.
    pub fn take_vec(source: &mut Vec<T>) -> Self {
        Self::from_vec(std::mem::take(source))
    }

    pub fn with_len_capacity(len: usize, capacity: usize) -> Self {
        Self::with_len_capacity_by(len, capacity, T::default)
    }
}

impl<T> Default for GorsSliceStorage<T> {
    fn default() -> Self {
        Self {
            backing: Vec::new(),
            start: 0,
            len: 0,
            capacity: 0,
        }
    }
}

impl<T: Clone> Clone for GorsSliceStorage<T> {
    fn clone(&self) -> Self {
        Self {
            backing: self.backing.clone(),
            start: self.start,
            len: self.len,
            capacity: self.capacity,
        }
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for GorsSliceStorage<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GorsSliceStorage")
            .field("visible", &self.visible())
            .field("capacity", &self.capacity)
            .finish()
    }
}

impl<T: PartialEq> PartialEq for GorsSliceStorage<T> {
    fn eq(&self, other: &Self) -> bool {
        self.visible() == other.visible()
    }
}

impl<T: Eq> Eq for GorsSliceStorage<T> {}

impl<T> std::ops::Deref for GorsSliceStorage<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.visible()
    }
}

impl<T> std::ops::DerefMut for GorsSliceStorage<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.visible_mut()
    }
}

impl<T> AsRef<[T]> for GorsSliceStorage<T> {
    fn as_ref(&self) -> &[T] {
        self.visible()
    }
}

impl<T> AsMut<[T]> for GorsSliceStorage<T> {
    fn as_mut(&mut self) -> &mut [T] {
        self.visible_mut()
    }
}

impl<T> Len for GorsSliceStorage<T> {
    fn len_value(&self) -> usize {
        self.len
    }
}

impl<T> Cap for GorsSliceStorage<T> {
    fn cap_value(&self) -> usize {
        self.capacity
    }
}

impl ByteSeq for GorsSliceStorage<u8> {
    fn byte_at(&self, index: usize) -> u8 {
        byte_at_or_panic(self.visible(), index)
    }

    fn byte_slice(&self, start: usize, end: usize) -> Vec<u8> {
        byte_slice_or_panic(self.visible(), start, end)
    }
}

impl<T: Default> From<Vec<T>> for GorsSliceStorage<T> {
    fn from(value: Vec<T>) -> Self {
        Self::from_vec(value)
    }
}

impl<T> From<GorsSliceStorage<T>> for Vec<T> {
    fn from(value: GorsSliceStorage<T>) -> Self {
        value.into_visible_vec()
    }
}

impl<T> IntoIterator for GorsSliceStorage<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_visible_vec().into_iter()
    }
}

impl<'a, T> IntoIterator for &'a GorsSliceStorage<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.visible().iter()
    }
}

impl<'a, T> IntoIterator for &'a mut GorsSliceStorage<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.visible_mut().iter_mut()
    }
}

impl<T> Extend<T> for GorsSliceStorage<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.extend_visible(iter);
    }
}

impl<T> FromIterator<T> for GorsSliceStorage<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let backing: Vec<T> = iter.into_iter().collect();
        let len = backing.len();
        Self::from_initialized_backing(backing, len)
    }
}

pub trait GorsOwnedSliceStorage<T> {
    fn gors_owned_slice_storage_mut(&mut self) -> &mut GorsSliceStorage<T>;
}

impl<T> GorsOwnedSliceStorage<T> for GorsSliceStorage<T> {
    fn gors_owned_slice_storage_mut(&mut self) -> &mut GorsSliceStorage<T> {
        self
    }
}

/// A borrowed Go slice header for callees that reslice beyond the argument's
/// visible length.
///
/// Ordinary mutable slice parameters can use `&mut [T]`, but Rust slices erase
/// the backing capacity that Go retains in a copied slice header. This wrapper
/// keeps a private header over a cloned view of the caller's backing storage and
/// writes mutations to the caller-visible range back when the call completes.
enum GorsSliceParamTarget<'a, T> {
    Visible(&'a mut [T]),
    Backing(&'a mut Vec<T>),
    Owned(&'a mut GorsSliceStorage<T>),
}

pub struct GorsSliceParam<'a, T: Clone + Default> {
    target: GorsSliceParamTarget<'a, T>,
    backing: Vec<T>,
    start: usize,
    len: usize,
    capacity: usize,
}

impl<'a, T: Clone + Default> GorsSliceParam<'a, T> {
    pub fn from_storage<S>(source: &'a mut S) -> Self
    where
        S: AsMut<[T]> + Cap + ?Sized,
    {
        let capacity = source.cap_value();
        let target = source.as_mut();
        let len = target.len();
        let mut backing = Vec::with_capacity(capacity);
        backing.extend_from_slice(target);
        backing.resize_with(capacity, Default::default);
        Self {
            target: GorsSliceParamTarget::Visible(target),
            backing,
            start: 0,
            len,
            capacity,
        }
    }

    pub fn from_param(source: &'a mut GorsSliceParam<'_, T>) -> Self {
        let backing = source.backing.clone();
        let start = source.start;
        let len = source.len;
        let capacity = source.capacity;
        Self {
            target: GorsSliceParamTarget::Backing(&mut source.backing),
            backing,
            start,
            len,
            capacity,
        }
    }

    /// Copy a parameter header over owned capacity-preserving storage.
    ///
    /// Header changes remain local to the parameter, while Drop writes the
    /// entire initialized capacity back to the owned storage so later caller
    /// reslices can reveal mutations beyond the old visible length.
    pub fn from_owned_storage<S>(source: &'a mut S) -> Self
    where
        S: GorsOwnedSliceStorage<T> + ?Sized,
    {
        let source = source.gors_owned_slice_storage_mut();
        let backing = source.full().to_vec();
        let len = source.len;
        let capacity = source.capacity;
        Self {
            target: GorsSliceParamTarget::Owned(source),
            backing,
            start: 0,
            len,
            capacity,
        }
    }

    pub fn reslice(&mut self, start: usize, end: usize, max: usize) {
        if start > end || end > max || max > self.capacity {
            panic_value("slice bounds out of range");
        }
        self.start += start;
        self.len = end - start;
        self.capacity = max - start;
    }

    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn clone(&self) -> Vec<T> {
        std::ops::Deref::deref(self).to_vec()
    }
}

impl<T: Clone + Default> std::ops::Deref for GorsSliceParam<'_, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.backing
            .get(self.start..self.start + self.len)
            .unwrap_or_else(|| panic_value("slice bounds out of range"))
    }
}

impl<T: Clone + Default> std::ops::DerefMut for GorsSliceParam<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.backing
            .get_mut(self.start..self.start + self.len)
            .unwrap_or_else(|| panic_value("slice bounds out of range"))
    }
}

impl<T: Clone + Default> AsRef<[T]> for GorsSliceParam<'_, T> {
    fn as_ref(&self) -> &[T] {
        std::ops::Deref::deref(self)
    }
}

impl<T: Clone + Default> AsMut<[T]> for GorsSliceParam<'_, T> {
    fn as_mut(&mut self) -> &mut [T] {
        std::ops::DerefMut::deref_mut(self)
    }
}

impl<T: Clone + Default> Len for GorsSliceParam<'_, T> {
    fn len_value(&self) -> usize {
        self.len
    }
}

impl<T: Clone + Default> Cap for GorsSliceParam<'_, T> {
    fn cap_value(&self) -> usize {
        self.capacity
    }
}

impl<T: Clone + Default> Drop for GorsSliceParam<'_, T> {
    fn drop(&mut self) {
        match &mut self.target {
            GorsSliceParamTarget::Visible(target) => {
                let target_len = target.len();
                let source = self
                    .backing
                    .get(..target_len)
                    .unwrap_or_else(|| panic_value("slice bounds out of range"));
                target.clone_from_slice(source);
            }
            GorsSliceParamTarget::Backing(target) => {
                target.clone_from_slice(&self.backing);
            }
            GorsSliceParamTarget::Owned(target) => {
                target.full_mut().clone_from_slice(&self.backing);
            }
        }
    }
}

pub trait Append<E> {
    fn append_value(self, elem: E) -> Self;
}

impl<T> Append<T> for Vec<T> {
    fn append_value(mut self, elem: T) -> Self {
        self.push(elem);
        self
    }
}

impl<T> Append<Vec<T>> for Vec<T> {
    fn append_value(mut self, elem: Vec<T>) -> Self {
        self.extend(elem);
        self
    }
}

impl Append<std::string::String> for Vec<u8> {
    fn append_value(mut self, elem: std::string::String) -> Self {
        self.extend(go_string_bytes(&elem));
        self
    }
}

impl Append<&str> for Vec<u8> {
    fn append_value(mut self, elem: &str) -> Self {
        self.extend(go_string_bytes(elem));
        self
    }
}

impl<T> Append<T> for GorsSliceStorage<T> {
    fn append_value(mut self, elem: T) -> Self {
        self.push_visible(elem);
        self
    }
}

impl<T> Append<Vec<T>> for GorsSliceStorage<T> {
    fn append_value(mut self, elem: Vec<T>) -> Self {
        self.extend_visible(elem);
        self
    }
}

impl<T> Append<GorsSliceStorage<T>> for GorsSliceStorage<T> {
    fn append_value(mut self, elem: GorsSliceStorage<T>) -> Self {
        self.extend_visible(elem);
        self
    }
}

impl Append<std::string::String> for GorsSliceStorage<u8> {
    fn append_value(mut self, elem: std::string::String) -> Self {
        self.extend_visible(go_string_bytes(&elem));
        self
    }
}

impl Append<&str> for GorsSliceStorage<u8> {
    fn append_value(mut self, elem: &str) -> Self {
        self.extend_visible(go_string_bytes(elem));
        self
    }
}

#[inline]
pub fn append<C, E>(v: C, elem: E) -> C
where
    C: Append<E>,
{
    v.append_value(elem)
}

#[inline]
pub fn append_slice<C, T>(mut v: C, elems: &[T]) -> C
where
    C: Extend<T>,
    T: Clone,
{
    v.extend(elems.iter().cloned());
    v
}

pub trait StringValue {
    fn string_value(self) -> std::string::String;
}

impl StringValue for Vec<u8> {
    fn string_value(self) -> std::string::String {
        go_string_from_bytes(&self)
    }
}

impl StringValue for &Vec<u8> {
    fn string_value(self) -> std::string::String {
        go_string_from_bytes(self)
    }
}

impl StringValue for GorsSliceStorage<u8> {
    fn string_value(self) -> std::string::String {
        go_string_from_bytes(self.visible())
    }
}

impl StringValue for &GorsSliceStorage<u8> {
    fn string_value(self) -> std::string::String {
        go_string_from_bytes(self.visible())
    }
}

fn go_string_from_runes(runes: impl IntoIterator<Item = i32>) -> std::string::String {
    let mut bytes = Vec::new();
    for rune in runes {
        let value = char::from_u32(rune as u32).unwrap_or('\u{fffd}');
        let mut encoded = [0u8; 4];
        bytes.extend_from_slice(value.encode_utf8(&mut encoded).as_bytes());
    }
    go_string_from_bytes(&bytes)
}

impl StringValue for Vec<i32> {
    fn string_value(self) -> std::string::String {
        go_string_from_runes(self)
    }
}

impl StringValue for &Vec<i32> {
    fn string_value(self) -> std::string::String {
        go_string_from_runes(self.iter().copied())
    }
}

impl StringValue for GorsSliceStorage<i32> {
    fn string_value(self) -> std::string::String {
        go_string_from_runes(self.visible().iter().copied())
    }
}

impl StringValue for &GorsSliceStorage<i32> {
    fn string_value(self) -> std::string::String {
        go_string_from_runes(self.visible().iter().copied())
    }
}

impl StringValue for std::string::String {
    fn string_value(self) -> std::string::String {
        self
    }
}

impl StringValue for &std::string::String {
    fn string_value(self) -> std::string::String {
        self.clone()
    }
}

impl StringValue for &str {
    fn string_value(self) -> std::string::String {
        self.to_string()
    }
}

impl StringValue for &[u8] {
    fn string_value(self) -> std::string::String {
        go_string_from_bytes(self)
    }
}

#[inline]
pub fn string<T: StringValue>(v: T) -> std::string::String {
    v.string_value()
}

#[inline]
pub fn go_string_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    go_string_bytes(left).cmp(&go_string_bytes(right))
}

#[inline]
pub fn copy_slice<D, S, T>(dst: &mut D, src: &S) -> usize
where
    D: AsMut<[T]> + ?Sized,
    S: AsRef<[T]> + ?Sized,
    T: Clone,
{
    let dst = dst.as_mut();
    let src = src.as_ref();
    let n = dst.len().min(src.len());
    if let (Some(dst), Some(src)) = (dst.get_mut(..n), src.get(..n)) {
        dst.clone_from_slice(src);
    }
    n
}

#[inline]
pub fn snapshot_slice<S, T>(src: &S) -> Vec<T>
where
    S: AsRef<[T]> + ?Sized,
    T: Clone,
{
    src.as_ref().to_vec()
}

#[inline]
pub fn copy<D, S, T>(dst: &mut D, src: &S) -> usize
where
    D: AsMut<[T]> + ?Sized,
    S: AsRef<[T]> + ?Sized,
    T: Clone,
{
    copy_slice(dst, src)
}

pub trait Delete<K> {
    fn delete_key(&mut self, key: &K);
}

impl<K: Hash + Eq, V> Delete<K> for HashMap<K, V> {
    fn delete_key(&mut self, key: &K) {
        self.remove(key);
    }
}

impl<K: Hash + Eq, V> Delete<K> for GorsMap<K, V> {
    fn delete_key(&mut self, key: &K) {
        self.delete(key);
    }
}

#[inline]
pub fn delete<K, M: Delete<K> + ?Sized>(m: &mut M, key: &K) {
    m.delete_key(key);
}

pub trait Clear {
    fn clear_value(&mut self);
}

impl<T: Default> Clear for Vec<T> {
    fn clear_value(&mut self) {
        for elem in self.iter_mut() {
            *elem = T::default();
        }
    }
}

impl<T: Default> Clear for [T] {
    fn clear_value(&mut self) {
        for elem in self.iter_mut() {
            *elem = T::default();
        }
    }
}

impl<T: Default> Clear for GorsSliceStorage<T> {
    fn clear_value(&mut self) {
        for elem in self.visible_mut() {
            *elem = T::default();
        }
    }
}

impl<K, V> Clear for HashMap<K, V> {
    fn clear_value(&mut self) {
        self.clear();
    }
}

impl<K, V> Clear for GorsMap<K, V> {
    fn clear_value(&mut self) {
        GorsMap::clear(self);
    }
}

#[inline]
pub fn clear<T: Clear + ?Sized>(v: &mut T) {
    v.clear_value();
}

#[inline]
pub fn clear_vec_range<T: Default>(
    (values, low, high, max): (&mut Vec<T>, usize, usize, Option<usize>),
) {
    let capacity = values.capacity();
    if low > high || high > capacity || max.is_some_and(|max| high > max || max > capacity) {
        panic_value("slice bounds out of range");
    }
    let initialized_high = high.min(values.len());
    if low < initialized_high {
        clear(&mut values[low..initialized_high]);
    }
}

#[inline]
pub fn r#new<T: Default>() -> Box<T> {
    Box::new(T::default())
}

#[inline]
pub fn new_box<T: Default>() -> Box<T> {
    r#new()
}

#[inline]
pub fn make_vec<T: Default + Clone>(size: usize) -> Vec<T> {
    vec![T::default(); size]
}

#[inline]
pub fn make_vec_cap<T>(cap: usize) -> Vec<T> {
    Vec::with_capacity(cap)
}

#[inline]
pub fn make_map<K, V>() -> GorsMap<K, V> {
    GorsMap::new()
}

#[inline]
pub fn make_map_cap<K, V>(cap: usize) -> GorsMap<K, V> {
    GorsMap::with_capacity(cap)
}

#[inline]
pub fn make_chan<T>(capacity: usize) -> Chan<T> {
    Chan::new(capacity)
}

#[inline]
pub fn max<T: PartialOrd>(a: T, b: T) -> T {
    if a >= b { a } else { b }
}

#[inline]
pub fn max3<T: PartialOrd>(a: T, b: T, c: T) -> T {
    max(max(a, b), c)
}

#[inline]
pub fn min<T: PartialOrd>(a: T, b: T) -> T {
    if a <= b { a } else { b }
}

#[inline]
pub fn min3<T: PartialOrd>(a: T, b: T, c: T) -> T {
    min(min(a, b), c)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Complex64 {
    pub re: f32,
    pub im: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Complex128 {
    pub re: f64,
    pub im: f64,
}

impl std::fmt::Display for Complex64 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}{:+}i)", self.re, self.im)
    }
}

impl std::fmt::Display for Complex128 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}{:+}i)", self.re, self.im)
    }
}

macro_rules! impl_complex_ops {
    ($ty:ty) => {
        impl std::ops::Add for $ty {
            type Output = Self;

            fn add(self, rhs: Self) -> Self {
                Self {
                    re: self.re + rhs.re,
                    im: self.im + rhs.im,
                }
            }
        }

        impl std::ops::Sub for $ty {
            type Output = Self;

            fn sub(self, rhs: Self) -> Self {
                Self {
                    re: self.re - rhs.re,
                    im: self.im - rhs.im,
                }
            }
        }

        impl std::ops::Mul for $ty {
            type Output = Self;

            fn mul(self, rhs: Self) -> Self {
                Self {
                    re: self.re.mul_add(rhs.re, -(self.im * rhs.im)),
                    im: self.re.mul_add(rhs.im, self.im * rhs.re),
                }
            }
        }

        impl std::ops::Div for $ty {
            type Output = Self;

            fn div(self, rhs: Self) -> Self {
                let denom = rhs.re.mul_add(rhs.re, rhs.im * rhs.im);
                Self {
                    re: self.re.mul_add(rhs.re, self.im * rhs.im) / denom,
                    im: self.im.mul_add(rhs.re, -(self.re * rhs.im)) / denom,
                }
            }
        }
    };
}

impl_complex_ops!(Complex64);
impl_complex_ops!(Complex128);

#[inline]
pub const fn complex64(re: f32, im: f32) -> Complex64 {
    Complex64 { re, im }
}

#[inline]
pub const fn complex128(re: f64, im: f64) -> Complex128 {
    Complex128 { re, im }
}

#[inline]
pub const fn complex(re: f64, im: f64) -> Complex128 {
    complex128(re, im)
}

pub trait Complex64Value {
    fn complex64_value(self) -> Complex64;
}

pub trait Complex128Value {
    fn complex128_value(self) -> Complex128;
}

impl Complex64Value for Complex64 {
    fn complex64_value(self) -> Complex64 {
        self
    }
}

impl Complex64Value for Complex128 {
    fn complex64_value(self) -> Complex64 {
        Complex64 {
            re: self.re as f32,
            im: self.im as f32,
        }
    }
}

impl Complex128Value for Complex128 {
    fn complex128_value(self) -> Complex128 {
        self
    }
}

impl Complex128Value for Complex64 {
    fn complex128_value(self) -> Complex128 {
        Complex128 {
            re: self.re as f64,
            im: self.im as f64,
        }
    }
}

macro_rules! impl_real_complex_conversions {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Complex64Value for $ty {
                fn complex64_value(self) -> Complex64 {
                    Complex64 { re: self as f32, im: 0.0 }
                }
            }

            impl Complex128Value for $ty {
                fn complex128_value(self) -> Complex128 {
                    Complex128 { re: self as f64, im: 0.0 }
                }
            }
        )*
    };
}

impl_real_complex_conversions!(f32, f64, isize, i8, i16, i32, i64, usize, u8, u16, u32, u64);

#[inline]
pub fn to_complex64<T: Complex64Value>(v: T) -> Complex64 {
    v.complex64_value()
}

#[inline]
pub fn to_complex128<T: Complex128Value>(v: T) -> Complex128 {
    v.complex128_value()
}

pub trait Real {
    type Output;

    fn real_value(self) -> Self::Output;
}

pub trait Imag {
    type Output;

    fn imag_value(self) -> Self::Output;
}

impl Real for Complex64 {
    type Output = f32;

    fn real_value(self) -> f32 {
        self.re
    }
}

impl Real for Complex128 {
    type Output = f64;

    fn real_value(self) -> f64 {
        self.re
    }
}

impl Imag for Complex64 {
    type Output = f32;

    fn imag_value(self) -> f32 {
        self.im
    }
}

impl Imag for Complex128 {
    type Output = f64;

    fn imag_value(self) -> f64 {
        self.im
    }
}

#[inline]
pub fn real<C: Real>(c: C) -> C::Output {
    c.real_value()
}

#[inline]
pub fn imag<C: Imag>(c: C) -> C::Output {
    c.imag_value()
}

#[inline]
pub fn real64(c: Complex64) -> f32 {
    c.re
}

#[inline]
pub fn real128(c: Complex128) -> f64 {
    c.re
}

#[inline]
pub fn imag64(c: Complex64) -> f32 {
    c.im
}

#[inline]
pub fn imag128(c: Complex128) -> f64 {
    c.im
}

pub trait BitcastFrom<T> {
    fn bitcast_from(value: T) -> Self;
}

impl BitcastFrom<f32> for u32 {
    fn bitcast_from(value: f32) -> Self {
        value.to_bits()
    }
}

impl BitcastFrom<u32> for f32 {
    fn bitcast_from(value: u32) -> Self {
        f32::from_bits(value)
    }
}

impl BitcastFrom<f64> for u64 {
    fn bitcast_from(value: f64) -> Self {
        value.to_bits()
    }
}

impl BitcastFrom<u64> for f64 {
    fn bitcast_from(value: u64) -> Self {
        f64::from_bits(value)
    }
}

#[inline]
pub fn bitcast_ref<T: Copy, U: BitcastFrom<T>>(value: &T) -> U {
    U::bitcast_from(*value)
}

struct ChanInner<T> {
    buf: VecDeque<T>,
    capacity: usize,
    closed: bool,
}

type ChanState<T> = Arc<(Mutex<ChanInner<T>>, Condvar, Condvar)>;

pub struct Chan<T> {
    inner: Option<ChanState<T>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TryRecvError {
    Empty,
}

impl<T> Clone for Chan<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> PartialEq for Chan<T> {
    fn eq(&self, other: &Self) -> bool {
        match (&self.inner, &other.inner) {
            (None, None) => true,
            (Some(left), Some(right)) => Arc::ptr_eq(left, right),
            _ => false,
        }
    }
}

impl<T> Eq for Chan<T> {}

impl<T> Default for Chan<T> {
    fn default() -> Self {
        Self { inner: None }
    }
}

impl<T> Chan<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Some(Arc::new((
                Mutex::new(ChanInner {
                    buf: VecDeque::with_capacity(capacity),
                    capacity,
                    closed: false,
                }),
                Condvar::new(),
                Condvar::new(),
            ))),
        }
    }

    #[allow(clippy::significant_drop_tightening)]
    pub fn send(&self, val: T) {
        let Some(inner) = self.inner.as_ref() else {
            let _ = val;
            loop {
                std::thread::park();
            }
        };
        let (lock, rx_cv, tx_cv) = &**inner;
        let mut inner = lock_chan(lock);
        if inner.closed {
            return;
        }
        while inner.capacity > 0 && inner.buf.len() >= inner.capacity {
            inner = wait_chan(tx_cv, inner);
            if inner.closed {
                return;
            }
        }
        if inner.capacity == 0 {
            inner.buf.push_back(val);
            rx_cv.notify_one();
            while !inner.buf.is_empty() && !inner.closed {
                inner = wait_chan(tx_cv, inner);
            }
        } else {
            inner.buf.push_back(val);
            rx_cv.notify_one();
        }
    }

    pub fn recv(&self) -> Option<T> {
        let Some(inner) = self.inner.as_ref() else {
            loop {
                std::thread::park();
            }
        };
        let (lock, rx_cv, tx_cv) = &**inner;
        let mut inner = lock_chan(lock);
        loop {
            if let Some(val) = inner.buf.pop_front() {
                tx_cv.notify_one();
                return Some(val);
            }
            if inner.closed {
                return None;
            }
            inner = wait_chan(rx_cv, inner);
        }
    }

    pub fn try_send(&self, val: T) -> Result<(), T> {
        let Some(inner) = self.inner.as_ref() else {
            return Err(val);
        };
        let (lock, rx_cv, _) = &**inner;
        let mut inner = lock_chan(lock);
        if inner.closed || inner.capacity == 0 || inner.buf.len() >= inner.capacity {
            return Err(val);
        }
        inner.buf.push_back(val);
        drop(inner);
        rx_cv.notify_one();
        Ok(())
    }

    pub fn try_recv(&self) -> Result<T, TryRecvError>
    where
        T: Default,
    {
        let Some(inner) = self.inner.as_ref() else {
            return Err(TryRecvError::Empty);
        };
        let (lock, _, tx_cv) = &**inner;
        let mut inner = lock_chan(lock);
        if let Some(val) = inner.buf.pop_front() {
            drop(inner);
            tx_cv.notify_one();
            Ok(val)
        } else if inner.closed {
            Ok(T::default())
        } else {
            Err(TryRecvError::Empty)
        }
    }

    pub fn try_recv_with_ok(&self) -> Option<(T, bool)>
    where
        T: Default,
    {
        let inner = self.inner.as_ref()?;
        let (lock, _, tx_cv) = &**inner;
        let mut inner = lock_chan(lock);
        if let Some(val) = inner.buf.pop_front() {
            drop(inner);
            tx_cv.notify_one();
            Some((val, true))
        } else if inner.closed {
            Some((T::default(), false))
        } else {
            None
        }
    }

    pub fn recv_with_ok(&self) -> (T, bool)
    where
        T: Default,
    {
        match self.recv() {
            Some(v) => (v, true),
            None => (T::default(), false),
        }
    }

    #[allow(clippy::significant_drop_tightening)]
    pub fn close(&self) {
        let Some(inner) = self.inner.as_ref() else {
            panic_value("close of nil channel");
        };
        let (lock, rx_cv, tx_cv) = &**inner;
        let mut inner = lock_chan(lock);
        if inner.closed {
            return;
        }
        inner.closed = true;
        rx_cv.notify_all();
        tx_cv.notify_all();
    }

    pub fn len(&self) -> usize {
        let Some(inner) = self.inner.as_ref() else {
            return 0;
        };
        let (lock, _, _) = &**inner;
        lock_chan(lock).buf.len()
    }

    pub fn is_empty(&self) -> bool {
        let Some(inner) = self.inner.as_ref() else {
            return true;
        };
        let (lock, _, _) = &**inner;
        lock_chan(lock).buf.is_empty()
    }

    pub fn cap(&self) -> usize {
        let Some(inner) = self.inner.as_ref() else {
            return 0;
        };
        let (lock, _, _) = &**inner;
        lock_chan(lock).capacity
    }

    pub fn is_nil(&self) -> bool {
        self.inner.is_none()
    }
}

pub struct ChanIter<T>(Chan<T>);

impl<T: Default> Iterator for ChanIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        let (val, ok) = self.0.recv_with_ok();
        ok.then_some(val)
    }
}

impl<T: Default> IntoIterator for Chan<T> {
    type Item = T;
    type IntoIter = ChanIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        ChanIter(self)
    }
}

fn lock_chan<T>(lock: &Mutex<ChanInner<T>>) -> MutexGuard<'_, ChanInner<T>> {
    match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn wait_chan<'a, T>(
    cvar: &Condvar,
    guard: MutexGuard<'a, ChanInner<T>>,
) -> MutexGuard<'a, ChanInner<T>> {
    match cvar.wait(guard) {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[inline]
pub fn close<T>(ch: &Chan<T>) {
    ch.close();
}

#[inline]
pub fn send<T>(ch: &Chan<T>, value: T) {
    ch.send(value);
}

#[inline]
pub fn recv<T: Default>(ch: &Chan<T>) -> T {
    ch.recv_with_ok().0
}

#[inline]
pub fn recv_with_ok<T: Default>(ch: &Chan<T>) -> (T, bool) {
    ch.recv_with_ok()
}

#[inline]
#[allow(clippy::panic)]
pub fn r#panic<T: Any + Send + 'static>(value: T) -> ! {
    std::panic::panic_any(value)
}

#[inline]
pub fn panic_value<T: Any + Send + 'static>(value: T) -> ! {
    r#panic(value)
}

fn any_box_to_send(value: Box<dyn Any>) -> Box<dyn Any + Send> {
    macro_rules! move_if {
        ($value:ident, $ty:ty) => {
            if $value.is::<$ty>() {
                match $value.downcast::<$ty>() {
                    Ok(v) => return v as Box<dyn Any + Send>,
                    Err(v) => $value = v,
                }
            }
        };
    }

    let mut value = value;
    move_if!(value, GorsReflectValue);
    move_if!(value, std::string::String);
    move_if!(value, &'static str);
    move_if!(value, bool);
    move_if!(value, isize);
    move_if!(value, i8);
    move_if!(value, i16);
    move_if!(value, i32);
    move_if!(value, i64);
    move_if!(value, usize);
    move_if!(value, u8);
    move_if!(value, u16);
    move_if!(value, u32);
    move_if!(value, u64);
    move_if!(value, f32);
    move_if!(value, f64);
    move_if!(value, Vec<u8>);
    move_if!(value, Vec<std::string::String>);

    clone_any_send_ref(value.as_ref())
}

#[inline]
pub fn panic_any_payload(value: Box<dyn Any>) -> ! {
    std::panic::resume_unwind(any_box_to_send(value))
}

#[inline]
pub fn set_recover_payload<T: Any + Send + 'static>(value: T) {
    RECOVER_PAYLOAD.with(|payload| *payload.borrow_mut() = Some(Box::new(value)));
}

#[inline]
pub fn set_recover_payload_any(value: Box<dyn Any>) {
    set_recover_payload_box(any_box_to_send(value));
}

#[inline]
pub fn set_recover_payload_box(mut value: Box<dyn Any + Send>) {
    if value.is::<UnrecoveredGoPanic>() {
        value = value
            .downcast::<UnrecoveredGoPanic>()
            .map(|wrapper| wrapper.0)
            .unwrap_or_else(|value| value);
    }
    RECOVER_PAYLOAD.with(|payload| *payload.borrow_mut() = Some(value));
}

#[inline]
pub fn recover() -> Box<dyn Any + Send> {
    RECOVER_PAYLOAD
        .with(|payload| payload.borrow_mut().take())
        .unwrap_or_else(|| Box::new(()))
}

#[inline]
pub fn resume_unrecovered_panic() {
    let payload = RECOVER_PAYLOAD.with(|payload| payload.borrow_mut().take());
    if let Some(payload) = payload {
        panic_unrecovered_payload(payload);
    }
}

#[inline]
#[allow(clippy::panic)]
fn panic_unrecovered_payload(payload: Box<dyn Any + Send>) -> ! {
    let payload = match payload.downcast::<std::string::String>() {
        Ok(value) => std::panic::panic_any(*value),
        Err(payload) => payload,
    };
    let payload = match payload.downcast::<&'static str>() {
        Ok(value) => std::panic::panic_any(*value),
        Err(payload) => payload,
    };
    std::panic::panic_any(UnrecoveredGoPanic(payload))
}

#[inline]
pub fn catch_go_unwind<F, R>(f: F) -> Result<R, Box<dyn Any + Send>>
where
    F: FnOnce() -> R,
{
    let _hook = GoPanicHookGuard::suppress();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
}

#[inline]
pub fn recover_func<F: FnOnce() + std::panic::UnwindSafe>(f: F) -> Option<std::string::String> {
    match std::panic::catch_unwind(f) {
        Ok(()) => None,
        Err(e) => {
            if let Some(s) = e.downcast_ref::<std::string::String>() {
                Some(s.clone())
            } else if let Some(s) = e.downcast_ref::<&str>() {
                Some(s.to_string())
            } else {
                Some("unknown panic".to_string())
            }
        }
    }
}

#[inline]
pub fn interface_is_nil(value: &dyn Any) -> bool {
    erased_any_payload(value).type_id() == TypeId::of::<()>()
}

#[inline]
pub fn print_empty() {}

#[inline]
pub fn println_empty() {
    ::std::eprintln!();
}

#[inline]
pub fn print_value<T: std::fmt::Display>(value: T) {
    ::std::eprint!("{value}");
}

#[inline]
pub fn println_value<T: std::fmt::Display>(value: T) {
    ::std::eprintln!("{value}");
}

fn write_go_string(value: std::string::String, newline: bool) {
    use std::io::Write as _;

    let bytes = go_string_bytes(&value);
    let stderr = std::io::stderr();
    let mut stderr = stderr.lock();
    let _ = stderr.write_all(&bytes);
    if newline {
        let _ = stderr.write_all(b"\n");
    }
}

#[inline]
pub fn print_go_string(value: std::string::String) {
    write_go_string(value, false);
}

#[inline]
pub fn println_go_string(value: std::string::String) {
    write_go_string(value, true);
}

pub fn format_slice<T: std::fmt::Display>(values: &[T]) -> std::string::String {
    let mut out = std::string::String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{value}");
    }
    out.push(']');
    out
}

#[macro_export]
macro_rules! print {
    () => {};
    ($($arg:expr),+ $(,)?) => {{
        $(
            eprint!("{}", $arg);
        )+
    }};
}

#[macro_export]
macro_rules! println {
    () => {
        eprintln!()
    };
    ($($arg:expr),+ $(,)?) => {{
        let mut first = true;
        $(
            if !first {
                eprint!(" ");
            }
            eprint!("{}", $arg);
            first = false;
        )+
        eprintln!();
    }};
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn builtin_type_aliases_match_go_widths() {
        let _: r#bool = r#true;
        let _: byte = 255;
        let _: rune = 'x' as i32;
        let _: int = -1;
        let _: uint = 1;
        let _: uintptr = 1;
        let _: float32 = 1.0;
        let _: float64 = 1.0;
        let _: string = "ok".to_string();
        let _: complex64 = complex64(1.0, 2.0);
        let _: complex128 = complex128(1.0, 2.0);
        let false_value = r#false;
        assert!(!false_value);
        assert_eq!(iota, 0);
        assert_eq!(nil, None);
    }

    #[test]
    fn boxed_errors_compare_by_nilness_and_message() {
        let nil_a: Box<dyn error> = Box::new(__GorsNooperror);
        let nil_b: Box<dyn error> = Box::new(__GorsNooperror);
        let err_a: Box<dyn error> = Box::new(__GorsStringError("same".to_string()));
        let err_b: Box<dyn error> = Box::new(__GorsStringError("same".to_string()));
        let err_c: Box<dyn error> = Box::new(__GorsStringError("other".to_string()));

        assert!(PartialEq::eq(&nil_a, &nil_b));
        assert!(PartialEq::eq(&err_a, &err_b));
        assert!(!PartialEq::eq(&nil_a, &err_c));
        assert!(!PartialEq::eq(&err_b, &err_c));
    }

    #[test]
    fn error_string_accepts_values_borrowed_through_lazy_lock() {
        static VALUE: std::sync::LazyLock<Box<dyn error>> =
            std::sync::LazyLock::new(|| Box::new(__GorsStringError("static error".to_string())));

        assert_eq!(error_string(&*VALUE), "static error");
    }

    #[test]
    fn projected_field_pointers_alias_owner_fields() {
        #[derive(Default)]
        struct Holder {
            value: isize,
            other: isize,
        }

        let owner = Arc::new(Mutex::new(Holder { value: 1, other: 2 }));
        let value_ptr = GorsPtr::from_arc_field(
            owner.clone(),
            std::mem::offset_of!(Holder, value),
            |holder: &mut Holder| &mut holder.value,
        );
        let same_value_ptr = GorsPtr::from_arc_field(
            owner.clone(),
            std::mem::offset_of!(Holder, value),
            |holder: &mut Holder| &mut holder.value,
        );
        let other_ptr = GorsPtr::from_arc_field(
            owner.clone(),
            std::mem::offset_of!(Holder, other),
            |holder: &mut Holder| &mut holder.other,
        );

        *value_ptr.lock().unwrap() = 7;

        assert_eq!(owner.lock().unwrap().value, 7);
        assert!(GorsPtr::ptr_eq(&value_ptr, &same_value_ptr));
        assert!(!GorsPtr::ptr_eq(&value_ptr, &other_ptr));
    }

    #[test]
    fn projected_pointer_field_pointers_alias_owner_fields() {
        #[derive(Default)]
        struct Holder {
            value: isize,
            other: isize,
        }

        let owner = GorsPtr::new(Holder { value: 1, other: 2 });
        let value_ptr = GorsPtr::from_ptr_field(
            owner.clone(),
            std::mem::offset_of!(Holder, value),
            |holder: &mut Holder| &mut holder.value,
        );
        let same_value_ptr = GorsPtr::from_ptr_field(
            owner.clone(),
            std::mem::offset_of!(Holder, value),
            |holder: &mut Holder| &mut holder.value,
        );
        let other_ptr = GorsPtr::from_ptr_field(
            owner.clone(),
            std::mem::offset_of!(Holder, other),
            |holder: &mut Holder| &mut holder.other,
        );

        *value_ptr.lock().unwrap() = 7;

        assert_eq!(owner.lock().unwrap().value, 7);
        assert!(GorsPtr::ptr_eq(&value_ptr, &same_value_ptr));
        assert!(!GorsPtr::ptr_eq(&value_ptr, &other_ptr));
    }

    #[test]
    fn projected_pointer_index_pointers_alias_owner_elements() {
        #[derive(Default)]
        struct Holder {
            values: [isize; 3],
        }

        let owner = GorsPtr::new(Holder { values: [1, 2, 3] });
        let value_ptr = GorsPtr::from_ptr_index(
            owner.clone(),
            std::mem::offset_of!(Holder, values),
            1,
            |holder: &mut Holder| &mut holder.values,
        );
        let same_value_ptr = GorsPtr::from_ptr_index(
            owner.clone(),
            std::mem::offset_of!(Holder, values),
            1,
            |holder: &mut Holder| &mut holder.values,
        );
        let other_ptr = GorsPtr::from_ptr_index(
            owner.clone(),
            std::mem::offset_of!(Holder, values),
            2,
            |holder: &mut Holder| &mut holder.values,
        );

        *value_ptr.lock().unwrap() = 7;

        assert_eq!(owner.lock().unwrap().values, [1, 7, 3]);
        assert!(GorsPtr::ptr_eq(&value_ptr, &same_value_ptr));
        assert!(!GorsPtr::ptr_eq(&value_ptr, &other_ptr));
    }

    #[test]
    fn projected_pointer_field_pointers_support_nonclone_fields() {
        struct NonClone {
            value: isize,
        }
        struct Holder {
            field: NonClone,
        }

        let owner = GorsPtr::new(Holder {
            field: NonClone { value: 1 },
        });
        let field_ptr = GorsPtr::from_ptr_field_identity(
            owner.clone(),
            std::mem::offset_of!(Holder, field),
            |holder: &mut Holder| &mut holder.field,
        );
        let same_field_ptr = GorsPtr::from_ptr_field_identity(
            owner.clone(),
            std::mem::offset_of!(Holder, field),
            |holder: &mut Holder| &mut holder.field,
        );

        assert!(GorsPtr::ptr_eq(&field_ptr, &same_field_ptr));
        assert_eq!(owner.lock().unwrap().field.value, 1);
    }

    #[test]
    fn projected_pointer_field_pointers_lock_default_nonclone_fields() {
        #[derive(Default)]
        struct NonClone {
            value: isize,
        }
        struct Holder {
            field: NonClone,
        }

        let owner = GorsPtr::new(Holder {
            field: NonClone { value: 1 },
        });
        let field_ptr = GorsPtr::from_ptr_field(
            owner.clone(),
            std::mem::offset_of!(Holder, field),
            |holder: &mut Holder| &mut holder.field,
        );

        field_ptr.lock().unwrap().value = 7;

        assert_eq!(owner.lock().unwrap().field.value, 7);
    }

    #[test]
    fn comparable_any_payloads_support_type_assertion_helpers() {
        let value = box_any_comparable(GorsPtr::new(7isize));

        assert!(any_is::<GorsPtr<isize>>(value.as_ref()));
        assert!(any_downcast_ref::<GorsPtr<isize>>(value.as_ref()).is_some());
    }

    #[test]
    fn go_strings_round_trip_invalid_utf8_and_escape_scalar() {
        let mut bytes = vec![0xff, b'a', 0xc3, 0xbf];
        bytes.extend_from_slice("\u{10ffff}".as_bytes());
        let encoded = go_string_from_bytes(&bytes);

        assert_eq!(go_string_bytes(&encoded), bytes);
        assert_eq!(len(&encoded), bytes.len());
        assert_eq!(byte_at(&encoded, 0), 0xff);
    }

    #[test]
    fn appending_go_strings_to_bytes_preserves_lossless_encoding() {
        let bytes = [0xff, b'a', 0xc3, 0xbf];
        let encoded = go_string_from_bytes(&bytes);

        assert_eq!(append(Vec::<u8>::new(), encoded.clone()), bytes);
        assert_eq!(append(Vec::<u8>::new(), encoded.as_str()), bytes);
    }

    #[test]
    fn byte_sequences_panic_for_out_of_bounds_indexes_and_slices() {
        for value in [
            Box::new(vec![b'a']) as Box<dyn ByteSeq>,
            Box::new(go_string_from_bytes(b"a")) as Box<dyn ByteSeq>,
        ] {
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| value.byte_at(1)))
                    .is_err()
            );
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| value.byte_slice(0, 2)))
                    .is_err()
            );
        }
    }

    #[test]
    fn go_string_range_matches_go_invalid_utf8_decoding() {
        let encoded = go_string_from_bytes(&[0xff, b'a', 0xc3, 0xbf]);

        assert_eq!(
            go_string_char_indices(&encoded).collect::<Vec<_>>(),
            vec![(0, 0xfffd), (1, i32::from(b'a')), (2, 0x00ff)]
        );
        assert_eq!(
            go_string_runes(&encoded),
            vec![0xfffd, i32::from(b'a'), 0x00ff]
        );
    }

    #[test]
    fn rune_slice_string_conversion_replaces_each_invalid_rune() {
        let runes = vec![-1, 0xd800, 0x110000, i32::from(b'a')];
        let expected = "\u{fffd}\u{fffd}\u{fffd}a";

        assert_eq!(string(runes.clone()), expected);
        assert_eq!(string(&runes), expected);
    }

    #[test]
    fn rune_slice_string_conversion_escapes_internal_abi_scalars() {
        let runes = vec![0x10ffff, 0xe080];
        let expected = "\u{10ffff}\u{e080}".as_bytes();

        assert_eq!(go_string_bytes(&string(runes.clone())), expected);
        assert_eq!(go_string_bytes(&string(&runes)), expected);
    }

    #[test]
    fn go_string_comparison_uses_raw_byte_order() {
        let lower_raw_byte = go_string_from_bytes(&[0x80]);
        let higher_utf8 = go_string_from_bytes("\u{00ff}".as_bytes());

        assert_eq!(
            go_string_cmp(&lower_raw_byte, &higher_utf8),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn len_and_cap_cover_sequences_maps_and_channels() {
        let values = vec![1, 2, 3];
        let array = [1, 2, 3, 4];
        let mut borrowed_values = [1, 2, 3, 4];
        let text = "hello".to_string();
        let mut map = HashMap::new();
        map.insert("a", 1);
        let ch: Chan<i32> = make_chan(2);
        ch.send(1);

        assert_eq!(len(&values), 3);
        assert_eq!(cap(&values), values.capacity());
        assert_eq!(len(&array), 4);
        assert_eq!(cap(&array), 4);
        let borrowed_slice = &mut borrowed_values[1..3];
        assert_eq!(cap(borrowed_slice), borrowed_slice.len());
        assert_eq!(len(&text), 5);
        assert_eq!(len(&map), 1);
        assert_eq!(len(&ch), 1);
        assert_eq!(cap(&ch), 2);
    }

    #[test]
    fn append_copy_delete_and_clear_match_builtin_shape() {
        let values = append(vec![1, 2], 3);
        assert_eq!(values, vec![1, 2, 3]);
        let values = append(values, vec![4, 5]);
        assert_eq!(values, vec![1, 2, 3, 4, 5]);

        let mut dst = vec![0, 0, 0];
        let src = vec![7, 8, 9, 10];
        assert_eq!(copy(&mut dst, &src), 3);
        assert_eq!(dst, vec![7, 8, 9]);

        let mut map = HashMap::new();
        map.insert("a", 1);
        map.insert("b", 2);
        delete(&mut map, &"a");
        assert_eq!(map.get("a"), None);
        clear(&mut map);
        assert!(map.is_empty());

        let mut cleared = vec![1, 2, 3];
        clear(&mut cleared);
        assert_eq!(cleared, vec![0, 0, 0]);
        let mut subrange = vec![1, 2, 3, 4];
        let subrange_slice = subrange.get_mut(1..3);
        assert!(subrange_slice.is_some());
        if let Some(slice) = subrange_slice {
            clear(slice);
        }
        assert_eq!(subrange, vec![1, 0, 0, 4]);
    }

    #[test]
    fn vec_slicing_preserves_capacity_and_zero_initialized_backing_values() {
        let mut values = Vec::with_capacity(4);
        values.extend([1, 2]);

        let shared = go_slice(&values, values.capacity(), 0, 1, values.capacity());
        assert_eq!(shared, vec![1]);
        assert_eq!(shared.capacity(), 4);

        let limited = go_slice(&values, values.capacity(), 0, 1, 1);
        assert_eq!(limited, vec![1]);
        assert_eq!(limited.capacity(), 1);

        let extended = go_slice(&values, values.capacity(), 0, 3, values.capacity());
        assert_eq!(extended, vec![1, 2, 0]);
        assert_eq!(extended.capacity(), 4);
    }

    #[test]
    fn borrowed_slice_headers_can_extend_and_write_back_visible_elements() {
        let mut values = Vec::with_capacity(4);
        values.extend([1, 2]);

        {
            let mut header = GorsSliceParam::from_storage(&mut values);
            header.reslice(0, 3, 4);
            *header.get_mut(0).unwrap() = 7;
            *header.get_mut(2).unwrap() = 9;
            assert_eq!(&*header, &[7, 2, 9]);
            assert_eq!(cap(&header), 4);
        }

        assert_eq!(values, vec![7, 2]);
        assert_eq!(values.capacity(), 4);
    }

    #[test]
    fn borrowed_slice_headers_forward_full_backing_without_sharing_header_changes() {
        let mut values = Vec::with_capacity(4);
        values.extend([1, 2]);

        {
            let mut outer = GorsSliceParam::from_storage(&mut values);
            {
                let mut inner = GorsSliceParam::from_param(&mut outer);
                inner.reslice(0, 3, 4);
                *inner.get_mut(2).unwrap() = 9;
            }

            assert_eq!(len(&outer), 2);
            outer.reslice(0, 3, 4);
            assert_eq!(outer.get(2), Some(&9));
        }

        assert_eq!(values, vec![1, 2]);
        assert_eq!(values.capacity(), 4);
    }

    #[test]
    fn owned_slice_storage_preserves_capacity_writes_after_reslice() {
        let mut values = Vec::with_capacity(4);
        values.extend([1, 2]);
        let original_capacity = values.capacity();
        let mut storage = GorsSliceStorage::take_vec(&mut values);

        assert!(values.is_empty());
        assert_eq!(storage.len(), 2);
        assert_eq!(storage.capacity(), original_capacity);
        storage
            .full_range_mut(2, 4, Some(4))
            .copy_from_slice(&[7, 8]);
        assert_eq!(storage.visible(), &[1, 2]);

        storage.reslice(0, 4, 4);
        assert_eq!(storage.visible(), &[1, 2, 7, 8]);

        values = storage.into_visible_vec();
        assert_eq!(values, [1, 2, 7, 8]);
        assert_eq!(values.capacity(), original_capacity);
    }

    #[test]
    fn owned_slice_parameter_writes_back_hidden_capacity_without_changing_header() {
        let mut values = Vec::with_capacity(4);
        values.extend([1, 2]);
        let mut storage = GorsSliceStorage::from_vec(values);

        {
            let mut parameter = GorsSliceParam::from_owned_storage(&mut storage);
            parameter.reslice(2, 4, 4);
            parameter.copy_from_slice(&[7, 8]);
        }

        assert_eq!(storage.visible(), &[1, 2]);
        assert_eq!(storage.full(), &[1, 2, 7, 8]);
        storage.reslice(0, 4, 4);
        assert_eq!(storage.visible(), &[1, 2, 7, 8]);
    }

    #[test]
    fn owned_slice_storage_checks_visible_and_full_slice_bounds() {
        let storage = GorsSliceStorage::from_initialized_backing(vec![1, 2, 3, 4], 2);

        assert_eq!(storage.visible_range(0, 2), &[1, 2]);
        assert_eq!(storage.full_range(2, 4, Some(4)), &[3, 4]);
        assert!(
            std::panic::catch_unwind(|| storage.visible_range(0, 3)).is_err(),
            "visible ranges must not extend to capacity"
        );
        assert!(
            std::panic::catch_unwind(|| storage.full_range(2, 4, Some(3))).is_err(),
            "the full-slice max must bound the high index"
        );
        assert!(
            std::panic::catch_unwind(|| storage.full_range(0, 2, Some(5))).is_err(),
            "the full-slice max must not exceed capacity"
        );
    }

    #[test]
    fn owned_slice_storage_drops_each_noncopy_backing_value_exactly_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct DropProbe {
            id: usize,
            drops: Arc<AtomicUsize>,
        }

        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::SeqCst);
            }
        }

        let drops = Arc::new(AtomicUsize::new(0));
        let backing = (0..4)
            .map(|id| DropProbe {
                id,
                drops: drops.clone(),
            })
            .collect();
        let mut storage = GorsSliceStorage::from_initialized_backing(backing, 2);
        storage.reslice(1, 3, 4);

        let visible = storage.into_visible_vec();
        assert_eq!(
            visible.iter().map(|probe| probe.id).collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(drops.load(Ordering::SeqCst), 2);

        drop(visible);
        assert_eq!(drops.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn owned_slice_storage_appends_elements_without_default() {
        struct NonDefault(&'static str);

        let mut storage = GorsSliceStorage::from_initialized_backing(
            vec![NonDefault("visible"), NonDefault("hidden")],
            1,
        );
        storage.push_visible(NonDefault("replaced"));
        assert_eq!(storage.capacity(), 2);
        assert_eq!(
            storage
                .visible()
                .iter()
                .map(|value| value.0)
                .collect::<Vec<_>>(),
            ["visible", "replaced"]
        );

        storage.extend_visible([NonDefault("grown-a"), NonDefault("grown-b")]);
        assert_eq!(storage.len(), 4);
        assert_eq!(storage.capacity(), 4);
        assert_eq!(
            storage
                .visible()
                .iter()
                .map(|value| value.0)
                .collect::<Vec<_>>(),
            ["visible", "replaced", "grown-a", "grown-b"]
        );
    }

    #[test]
    fn owned_slice_storage_accepts_compiler_supplied_zero_values() {
        let mut next = 0_usize;
        let storage = GorsSliceStorage::with_len_capacity_by(2, 4, || {
            let value = next;
            next += 1;
            Box::new(value) as Box<dyn Any>
        });

        assert_eq!(storage.len(), 2);
        assert_eq!(storage.capacity(), 4);
        assert_eq!(next, 4);
        assert_eq!(
            storage
                .full()
                .iter()
                .filter_map(|value| value.as_ref().downcast_ref::<usize>().copied())
                .collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
    }

    #[test]
    fn make_new_max_min_and_string_conversion_work() {
        let boxed: Box<i32> = r#new();
        assert_eq!(*boxed, 0);
        assert_eq!(make_vec::<i32>(3), vec![0, 0, 0]);
        assert_eq!(make_vec_cap::<i32>(5).capacity(), 5);
        assert!(make_map::<String, i32>().is_empty());
        assert_eq!(make_map_cap::<String, i32>(5).capacity(), 7);
        assert_eq!(max(2, 5), 5);
        assert_eq!(max3(2, 5, 4), 5);
        assert_eq!(min(2, 5), 2);
        assert_eq!(min3(2, 5, 4), 2);
        assert_eq!(string(vec![104, 105]), "hi");
        assert_eq!(string("hi"), "hi");
    }

    #[test]
    fn go_maps_preserve_nil_state_and_shared_identity() {
        let nil_map: GorsMap<String, isize> = GorsMap::default();
        assert!(nil_map.is_nil());
        assert_eq!(nil_map.len(), 0);
        assert_eq!(
            nil_map.get_with(&"missing".to_string(), |value| value.copied()),
            None
        );
        nil_map.delete(&"missing".to_string());
        nil_map.clear();

        let map = GorsMap::from([("value".to_string(), 1_isize)]);
        let alias = map.clone();
        alias.insert("value".to_string(), 2);
        assert_eq!(
            map.get_with(&"value".to_string(), |value| value.copied()),
            Some(2)
        );
    }

    #[test]
    fn nil_map_updates_panic_but_deep_clones_are_independent() {
        let nil_map: GorsMap<String, isize> = GorsMap::default();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            nil_map.insert("missing".to_string(), 1);
        }));
        assert!(panic.is_err());

        let map = GorsMap::from([("value".to_string(), 1_isize)]);
        let independent = map.deep_clone();
        independent.insert("value".to_string(), 3);
        assert_eq!(
            map.get_with(&"value".to_string(), |value| value.copied()),
            Some(1)
        );
        assert_eq!(
            independent.get_with(&"value".to_string(), |value| value.copied()),
            Some(3)
        );
    }

    #[test]
    fn reflect_kind_checks_cover_direct_and_boxed_values() {
        assert!(reflect_kind_is(
            &"go".to_string(),
            __GorsReflectKind::String
        ));
        assert!(reflect_kind_is(&true, __GorsReflectKind::Bool));
        assert!(reflect_kind_is(&1_isize, __GorsReflectKind::Int));
        assert!(reflect_kind_is(&vec![1, 2], __GorsReflectKind::Slice));

        let boxed_string = Box::new("go".to_string()) as Box<dyn Any>;
        let boxed_int = Box::new(1_isize) as Box<dyn Any>;
        assert!(reflect_kind_is(&boxed_string, __GorsReflectKind::String));
        assert!(reflect_kind_is(&boxed_int, __GorsReflectKind::Int));
        assert!(!reflect_kind_is(&boxed_int, __GorsReflectKind::String));
    }

    #[test]
    fn byte_sequence_helpers_cover_strings_and_byte_slices() {
        let text = "gors".to_string();
        let literal = "gors";
        let bytes = vec![b'g', b'o', b'r', b's'];

        assert_eq!(byte_at(&text, 1), b'o');
        assert_eq!(byte_at(literal, 1), b'o');
        assert_eq!(byte_at(&bytes, 2), b'r');
        assert_eq!(byte_at(bytes.as_slice(), 3), b's');
        assert_eq!(byte_slice(&text, 1, 3), vec![b'o', b'r']);
        assert_eq!(byte_slice(literal, 1, 3), vec![b'o', b'r']);
        assert_eq!(byte_slice(&bytes, 0, 2), vec![b'g', b'o']);
        assert_eq!(byte_slice(bytes.as_slice(), 2, 4), vec![b'r', b's']);
        assert_eq!(string_from_byte_seq(&text), "gors");
        assert_eq!(string_from_byte_seq(literal), "gors");
        assert_eq!(string_from_byte_seq(&bytes), "gors");
        assert_eq!(string_from_byte_seq(bytes.as_slice()), "gors");

        let encoded = go_string_from_bytes(&[0xff, b'o']);
        let borrowed = encoded.as_str();
        assert_eq!(len(borrowed), 2);
        assert_eq!(byte_at(borrowed, 0), 0xff);

        let mut mutable = bytes;
        let mutable_slice = mutable.as_mut_slice();
        assert_eq!(string_from_byte_seq(&mutable_slice), "gors");
    }

    #[test]
    fn lock_func_calls_shared_function_values() {
        let func: Arc<Mutex<dyn FnMut(isize) -> isize + Send>> =
            Arc::new(Mutex::new(|value| value + 1));
        let result = {
            let mut locked = lock_func(&func);
            (*locked)(41)
        };
        assert_eq!(result, 42);
    }

    #[test]
    fn complex_real_imag_and_bitcasts_work() {
        let c64 = complex64(1.0, 2.0);
        let c128 = complex128(3.0, 4.0);
        assert_eq!(real(c64), 1.0);
        assert_eq!(imag(c64), 2.0);
        assert_eq!(real(c128), 3.0);
        assert_eq!(imag(c128), 4.0);
        assert_eq!(real128(complex(5.0, 6.0)), 5.0);
        assert_eq!(to_complex64(7.0_f32).re, 7.0);
        assert_eq!(to_complex128(8_i32).re, 8.0);

        let value = 1.5_f32;
        let bits: u32 = bitcast_ref(&value);
        assert_eq!(f32::from_bits(bits), value);
    }

    #[test]
    fn channel_send_receive_close_and_iteration_work() {
        let ch = make_chan(1);
        send(&ch, 42);
        assert_eq!(recv(&ch), 42);
        send(&ch, 7);
        assert_eq!(recv_with_ok(&ch), (7, true));
        close(&ch);
        assert_eq!(recv_with_ok::<i32>(&ch), (0, false));

        let iter_ch = make_chan(2);
        send(&iter_ch, 1);
        send(&iter_ch, 2);
        close(&iter_ch);
        assert_eq!(iter_ch.into_iter().collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn channel_try_helpers_are_non_blocking() {
        let ch = make_chan(1);
        assert_eq!(ch.try_recv(), Err(TryRecvError::Empty));
        assert_eq!(ch.try_send(1), Ok(()));
        assert_eq!(ch.try_send(2), Err(2));
        assert_eq!(ch.try_recv(), Ok(1));
        assert_eq!(ch.try_recv(), Err(TryRecvError::Empty));
        close(&ch);
        assert_eq!(ch.try_recv_with_ok(), Some((0, false)));
        assert_eq!(ch.try_recv(), Ok(0));

        let unbuffered = make_chan(0);
        assert_eq!(unbuffered.try_send(3), Err(3));
    }

    #[test]
    fn nil_channels_are_distinct_from_made_channels() {
        let nil_ch: Chan<i32> = Chan::default();
        assert!(nil_ch.is_nil());
        assert_eq!(len(&nil_ch), 0);
        assert_eq!(cap(&nil_ch), 0);
        assert_eq!(nil_ch.try_recv(), Err(TryRecvError::Empty));
        assert_eq!(nil_ch.try_recv_with_ok(), None);
        assert_eq!(nil_ch.try_send(1), Err(1));

        let made = make_chan::<i32>(0);
        assert!(!made.is_nil());
        assert!(!PartialEq::eq(&nil_ch, &made));
        assert!(PartialEq::eq(
            &Chan::<i32>::default(),
            &Chan::<i32>::default()
        ));
    }

    #[test]
    fn reflect_type_comparable_tracks_known_non_comparable_values() {
        assert!(reflect_type_comparable(
            (Box::new("key".to_string()) as Box<dyn Any>).as_ref()
        ));
        assert!(reflect_type_comparable(
            (Box::new(42_isize) as Box<dyn Any>).as_ref()
        ));
        assert!(!reflect_type_comparable(
            (Box::new(vec![1_u8, 2]) as Box<dyn Any>).as_ref()
        ));
        let storage = GorsSliceStorage::from_initialized_backing(vec![1_u8, 2, 3], 2);
        assert_eq!(reflect_value_len(&storage), 2);
        assert!(!reflect_type_comparable(&storage));
        assert!(!reflect_type_comparable(
            (Box::new(()) as Box<dyn Any>).as_ref()
        ));
    }

    #[derive(Clone, PartialEq)]
    struct NamedString(String);

    #[test]
    fn comparable_any_preserves_named_value_equality() {
        let left = box_any_comparable(NamedString("name".to_string()));
        let same = box_any_comparable(NamedString("name".to_string()));
        let other = box_any_comparable(NamedString("other".to_string()));

        assert!(any_eq(left.as_ref(), same.as_ref()));
        assert!(!any_eq(left.as_ref(), other.as_ref()));

        let cloned = clone_any(left.as_ref());
        assert!(any_downcast_ref::<NamedString>(cloned.as_ref()).is_some());
        assert!(any_eq(cloned.as_ref(), same.as_ref()));
    }

    #[derive(Clone, PartialEq)]
    struct OtherNamedString(String);

    #[test]
    fn interface_keys_compare_owned_values_by_dynamic_type_and_value() {
        let left = GorsInterfaceKey::for_comparable(&NamedString("name".to_string()));
        let same = GorsInterfaceKey::for_comparable(&NamedString("name".to_string()));
        let unequal = GorsInterfaceKey::for_comparable(&NamedString("other".to_string()));
        let other_type = GorsInterfaceKey::for_comparable(&OtherNamedString("name".to_string()));

        assert_eq!(left, same);
        assert_ne!(left, unequal);
        assert_ne!(left, other_type);
    }

    #[test]
    fn interface_pointer_keys_preserve_type_identity_and_typed_nil() {
        let value = 7_isize;
        let pointer = std::ptr::from_ref(&value).cast::<()>();
        let same = GorsInterfaceKey::for_ptr::<isize>(pointer);
        let different_dynamic_type = GorsInterfaceKey::for_ptr::<usize>(pointer);
        let typed_nil = GorsInterfaceKey::for_ptr::<isize>(std::ptr::null());

        assert_eq!(GorsInterfaceKey::for_ptr::<isize>(pointer), same);
        assert_ne!(same, different_dynamic_type);
        assert_ne!(typed_nil, GorsInterfaceKey::nil());
        assert_eq!(
            typed_nil,
            GorsInterfaceKey::for_ptr::<isize>(std::ptr::null())
        );
    }

    #[test]
    fn non_comparable_interface_keys_defer_panics_until_use() {
        let left = GorsInterfaceKey::non_comparable::<Vec<isize>>();
        let right = GorsInterfaceKey::non_comparable::<Vec<isize>>();
        let other_type = GorsInterfaceKey::non_comparable::<Vec<usize>>();

        assert_ne!(left, other_type);
        let compare = catch_go_unwind(|| left == right);
        assert!(compare.is_err());

        let conservative = GorsInterfaceKey::non_comparable::<String>();
        let comparable = GorsInterfaceKey::for_comparable(&String::from("value"));
        assert!(catch_go_unwind(|| conservative == comparable).is_err());

        let key = GorsInterfaceKey::non_comparable::<Vec<isize>>();
        let hash = catch_go_unwind(|| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            key.hash(&mut hasher);
        });
        assert!(hash.is_err());
    }

    #[test]
    fn comparable_any_clones_preserve_reflect_comparability() {
        let original = box_any_comparable(NamedString("name".to_string()));
        let cloned = clone_any(original.as_ref());

        assert!(reflect_type_comparable(cloned.as_ref()));
        assert_eq!(
            any_downcast_ref::<NamedString>(cloned.as_ref()).map(|value| value.0.as_str()),
            Some("name")
        );
    }

    #[test]
    fn send_sync_comparable_any_clones_stay_recloneable() {
        let original = box_any_comparable(NamedString("name".to_string()));
        let lookup = box_any_comparable(NamedString("name".to_string()));

        let stored = clone_any_send_sync(original.as_ref());
        let first_read = clone_any(stored.as_ref());
        assert!(any_eq(first_read.as_ref(), lookup.as_ref()));
        assert_eq!(
            any_downcast_ref::<NamedString>(first_read.as_ref()).map(|value| value.0.as_str()),
            Some("name")
        );

        let stored_again = clone_any_send_sync(stored.as_ref());
        let second_read = clone_any(stored_again.as_ref());
        assert!(any_eq(second_read.as_ref(), lookup.as_ref()));
    }

    #[derive(Clone)]
    struct NamedCallback(Arc<dyn Fn(isize) -> isize + Send + Sync>);

    #[test]
    fn erased_clone_traits_keep_local_and_send_sync_capabilities_distinct() {
        fn assert_send_sync<T: Send + Sync>() {}

        trait AmbiguousIfSend<A> {
            fn marker() {}
        }
        impl<T: ?Sized> AmbiguousIfSend<()> for T {}
        impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}

        trait AmbiguousIfSync<A> {
            fn marker() {}
        }
        impl<T: ?Sized> AmbiguousIfSync<()> for T {}
        impl<T: ?Sized + Sync> AmbiguousIfSync<u8> for T {}

        assert_send_sync::<Box<dyn GorsAnyClone>>();
        assert_send_sync::<Box<dyn GorsAnyComparable>>();
        let _ = <Box<dyn GorsAnyLocalClone> as AmbiguousIfSend<_>>::marker;
        let _ = <Box<dyn GorsAnyLocalClone> as AmbiguousIfSync<_>>::marker;
        let _ = <Box<dyn GorsAnyLocalComparable> as AmbiguousIfSend<_>>::marker;
        let _ = <Box<dyn GorsAnyLocalComparable> as AmbiguousIfSync<_>>::marker;
    }

    #[test]
    fn local_erased_values_clone_without_crossing_send_boundaries() {
        #[derive(Clone, PartialEq)]
        struct LocalValue(std::rc::Rc<isize>);

        let clone_only = box_any_local_clone(LocalValue(std::rc::Rc::new(41)));
        let cloned = clone_any(clone_only.as_ref());
        assert_eq!(
            any_downcast_ref::<LocalValue>(cloned.as_ref()).map(|value| *value.0),
            Some(41)
        );
        assert!(clone_any_send_sync(clone_only.as_ref()).is::<()>());

        let comparable = box_any_local_comparable(LocalValue(std::rc::Rc::new(42)));
        let lookup = box_any_local_comparable(LocalValue(std::rc::Rc::new(42)));
        let cloned = clone_any(comparable.as_ref());
        assert!(any_eq(cloned.as_ref(), lookup.as_ref()));
        assert!(reflect_type_comparable(cloned.as_ref()));
        assert!(clone_any_send_sync(comparable.as_ref()).is::<()>());
    }

    #[test]
    fn clone_only_any_values_survive_erased_send_sync_round_trips() {
        let original = box_any_clone(NamedCallback(Arc::new(|value| value + 1)));
        assert!(any_is::<NamedCallback>(original.as_ref()));
        assert!(!reflect_type_comparable(original.as_ref()));

        let stored = clone_any_send_sync(original.as_ref());
        let sent = clone_any_send_ref(stored.as_ref());
        let loaded = clone_any(sent.as_ref());
        let callback = any_downcast_ref::<NamedCallback>(loaded.as_ref());

        assert!(callback.is_some());
        assert_eq!(callback.map(|callback| (callback.0)(41)), Some(42));
        assert!(!reflect_type_comparable(loaded.as_ref()));

        let stored_again = clone_any_send_sync(loaded.as_ref());
        let loaded_again = clone_any(stored_again.as_ref());
        assert_eq!(
            any_downcast_ref::<NamedCallback>(loaded_again.as_ref())
                .map(|callback| (callback.0)(1)),
            Some(2)
        );
    }

    #[test]
    fn erased_any_payload_unwraps_nested_clone_only_boxes() {
        let wrapped = box_any_clone(NamedCallback(Arc::new(|value| value * 2)));
        let nested = Box::new(wrapped) as Box<dyn Any>;

        assert_eq!(
            any_downcast_ref::<NamedCallback>(nested.as_ref()).map(|callback| (callback.0)(21)),
            Some(42)
        );
    }

    #[test]
    fn erased_dynamic_type_identity_uses_payload_and_preserves_nil() {
        #[derive(Clone)]
        struct CloneOnlyA;
        #[derive(Clone)]
        struct CloneOnlyB;
        #[derive(Clone, PartialEq)]
        struct ComparableA;

        let clone_a = box_any_clone(CloneOnlyA);
        let clone_b = box_any_clone(CloneOnlyB);
        let comparable_a = box_any_comparable(ComparableA);
        let nested = Box::new(clone_a) as Box<dyn Any>;

        assert_eq!(
            any_dynamic_type_id(nested.as_ref()),
            Some(TypeId::of::<CloneOnlyA>())
        );
        assert_eq!(
            any_dynamic_type_id(clone_b.as_ref()),
            Some(TypeId::of::<CloneOnlyB>())
        );
        assert_eq!(
            any_dynamic_type_id(comparable_a.as_ref()),
            Some(TypeId::of::<ComparableA>())
        );
        assert_ne!(
            any_dynamic_type_id(nested.as_ref()),
            any_dynamic_type_id(clone_b.as_ref())
        );
        assert_eq!(any_dynamic_type_id(&() as &dyn Any), None);
    }

    #[test]
    fn errors_erased_as_any_expose_their_concrete_dynamic_value_and_nil() {
        #[derive(Clone, PartialEq)]
        struct ConcreteError(&'static str);

        impl error for ConcreteError {
            fn __gors_as_any(&self) -> Option<&dyn Any> {
                Some(self)
            }

            fn __gors_interface_key(&self) -> GorsInterfaceKey {
                GorsInterfaceKey::for_comparable(self)
            }

            fn __gors_clone_box(&self) -> Box<dyn error> {
                Box::new(self.clone())
            }

            fn Error(&self) -> String {
                self.0.to_string()
            }
        }

        let concrete =
            Box::new(Box::new(ConcreteError("concrete")) as Box<dyn error>) as Box<dyn Any>;
        let nil_any = Box::new(Box::new(__GorsNooperror) as Box<dyn error>) as Box<dyn Any>;

        assert!(any_is::<ConcreteError>(concrete.as_ref()));
        assert_eq!(
            any_downcast_ref::<ConcreteError>(concrete.as_ref()).map(|error| error.0),
            Some("concrete")
        );
        assert_eq!(
            any_dynamic_type_id(concrete.as_ref()),
            Some(TypeId::of::<ConcreteError>())
        );
        assert!(interface_is_nil(nil_any.as_ref()));
        assert_eq!(any_dynamic_type_id(nil_any.as_ref()), None);
    }

    #[test]
    fn equal_dynamic_clone_only_any_values_panic_as_non_comparable() {
        #[derive(Clone)]
        struct CloneOnly(Vec<isize>);
        #[derive(Clone)]
        struct OtherCloneOnly(Vec<isize>);

        let left = box_any_clone(CloneOnly(vec![1]));
        let right = box_any_clone(CloneOnly(vec![1]));
        let other = box_any_clone(OtherCloneOnly(vec![1]));

        assert!(!any_eq(left.as_ref(), other.as_ref()));
        assert!(catch_go_unwind(|| any_eq(left.as_ref(), right.as_ref())).is_err());
    }

    #[test]
    fn cloned_errors_preserve_concrete_dynamic_identity() {
        #[derive(Clone, PartialEq)]
        struct WrappedError(&'static str);

        impl error for WrappedError {
            fn __gors_as_any(&self) -> Option<&dyn Any> {
                Some(self)
            }

            fn __gors_interface_key(&self) -> GorsInterfaceKey {
                GorsInterfaceKey::for_comparable(self)
            }

            fn __gors_clone_box(&self) -> Box<dyn error> {
                Box::new(self.clone())
            }

            fn Error(&self) -> String {
                self.0.to_string()
            }
        }

        let original = Box::new(WrappedError("wrapped")) as Box<dyn error>;
        let cloned = original.clone();
        let cloned_again = cloned.clone();
        let same_message_other_type =
            Box::new(__GorsStringError("wrapped".to_string())) as Box<dyn error>;

        assert!(PartialEq::eq(&original, &cloned));
        assert!(PartialEq::eq(&cloned, &cloned_again));
        assert!(!PartialEq::eq(&original, &same_message_other_type));
        assert!(
            cloned
                .__gors_as_any()
                .is_some_and(|value| value.is::<WrappedError>())
        );
        assert!(
            cloned_again
                .__gors_as_any()
                .is_some_and(|value| value.is::<WrappedError>())
        );
        assert_eq!(cloned_again.Error(), "wrapped");
    }

    #[test]
    fn panic_and_recover_helpers_have_defined_behavior() {
        assert!(interface_is_nil(recover().as_ref()));
        set_recover_payload("boom".to_string());
        let recovered = recover();
        assert!(!interface_is_nil(recovered.as_ref()));
        assert!(interface_is_nil(recover().as_ref()));
        set_recover_payload_any(Box::new("stored".to_string()) as Box<dyn Any>);
        let recovered_any = recover();
        assert_eq!(
            recovered_any.downcast_ref::<std::string::String>(),
            Some(&"stored".to_string())
        );
        assert_eq!(recover_func(|| {}).as_deref(), None);
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        assert_eq!(
            recover_func(|| panic_value("boom")).as_deref(),
            Some("boom")
        );
        std::panic::set_hook(previous_hook);
        let send_any = Box::new("clone".to_string()) as Box<dyn Any + Send>;
        let cloned = clone_any(&*send_any);
        assert_eq!(
            cloned.downcast_ref::<std::string::String>(),
            Some(&"clone".to_string())
        );
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            panic_any_payload(Box::new("payload".to_string()) as Box<dyn Any>);
        }));
        let payload = panic_result.unwrap_err();
        assert_eq!(
            payload.downcast_ref::<std::string::String>(),
            Some(&"payload".to_string())
        );
        std::panic::set_hook(previous_hook);
    }
}
