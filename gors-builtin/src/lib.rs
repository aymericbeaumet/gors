#![allow(
    dead_code,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals
)]

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct GorsInterfaceKey {
    type_name: &'static str,
    data: usize,
    field_key: usize,
}

impl GorsInterfaceKey {
    pub fn nil() -> Self {
        Self::default()
    }

    pub fn for_ptr<T>(ptr: *const ()) -> Self {
        Self {
            type_name: std::any::type_name::<T>(),
            data: ptr as usize,
            field_key: 0,
        }
    }

    pub fn for_projected_ptr<T>(owner: *const (), field_key: usize) -> Self {
        Self {
            type_name: std::any::type_name::<T>(),
            data: owner as usize,
            field_key,
        }
    }

    pub fn non_comparable() -> Self {
        panic_value("hash of unhashable type")
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
        comparable_any_payload(other)
            .and_then(|other| other.downcast_ref::<T>())
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

fn comparable_any(value: &dyn Any) -> Option<&dyn GorsAnyComparable> {
    value
        .downcast_ref::<Box<dyn GorsAnyComparable>>()
        .map(|value| &**value)
}

fn comparable_any_payload(value: &dyn Any) -> Option<&dyn Any> {
    comparable_any(value)
        .map(GorsAnyComparable::as_any)
        .or(Some(value))
}

pub fn any_is<T: Any>(value: &dyn Any) -> bool {
    comparable_any_payload(value).is_some_and(|value| value.is::<T>())
}

pub fn any_downcast_ref<T: Any>(value: &dyn Any) -> Option<&T> {
    comparable_any_payload(value).and_then(|value| value.downcast_ref::<T>())
}

pub trait error: Send + Sync {
    fn __gors_as_any(&self) -> Option<&dyn Any>;
    fn __gors_interface_key(&self) -> GorsInterfaceKey;
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

    fn Error(&self) -> std::string::String {
        std::string::String::new()
    }
}

#[derive(Clone, Default)]
pub struct __GorsStringError(pub std::string::String);

impl error for __GorsStringError {
    fn __gors_as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }

    fn __gors_interface_key(&self) -> GorsInterfaceKey {
        GorsInterfaceKey::non_comparable()
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
        if self.__gors_as_any().is_none() {
            Box::new(__GorsNooperror)
        } else {
            Box::new(__GorsStringError(self.Error()))
        }
    }
}

impl PartialEq for Box<dyn error> {
    fn eq(&self, other: &Self) -> bool {
        match (
            self.__gors_as_any().is_none(),
            other.__gors_as_any().is_none(),
        ) {
            (true, true) => true,
            (true, false) | (false, true) => false,
            (false, false) => self.Error() == other.Error(),
        }
    }
}

impl error for Box<dyn error> {
    fn __gors_as_any(&self) -> Option<&dyn Any> {
        (**self).__gors_as_any()
    }

    fn __gors_interface_key(&self) -> GorsInterfaceKey {
        (**self).__gors_interface_key()
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
    pub fn slice<T: 'static + Send>(slice: Arc<Mutex<Vec<T>>>) -> Self {
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

struct GorsReflectSlice<T> {
    slice: Arc<Mutex<Vec<T>>>,
}

impl<T: 'static + Send> GorsReflectOps for GorsReflectSlice<T> {
    fn kind(&self) -> __GorsReflectKind {
        __GorsReflectKind::Slice
    }

    fn len(&self) -> isize {
        self.slice
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len() as isize
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
        if i >= slice.len() || j >= slice.len() {
            panic_value("reflect: slice index out of range");
        }
        slice.swap(i, j);
    }
}

fn lock_reflect_ops(
    ops: &Arc<Mutex<Box<dyn GorsReflectOps>>>,
) -> MutexGuard<'_, Box<dyn GorsReflectOps>> {
    ops.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn reflect_slice_any<T: 'static + Send>(slice: Arc<Mutex<Vec<T>>>) -> Box<dyn Any> {
    Box::new(GorsReflectValue::slice(slice)) as Box<dyn Any>
}

pub fn reflect_value_kind(value: &dyn Any) -> __GorsReflectKind {
    if let Some(value) = value.downcast_ref::<GorsReflectValue>() {
        return value.kind();
    }
    reflect_kind_of_any(value)
}

pub fn reflect_value_len(value: &dyn Any) -> isize {
    if let Some(value) = value.downcast_ref::<GorsReflectValue>() {
        return value.len();
    }
    macro_rules! len_if_vec {
        ($ty:ty) => {
            if let Some(value) = value.downcast_ref::<Vec<$ty>>() {
                return value.len() as isize;
            }
        };
    }
    len_if_vec!(std::string::String);
    len_if_vec!(bool);
    len_if_vec!(isize);
    len_if_vec!(i8);
    len_if_vec!(i16);
    len_if_vec!(i32);
    len_if_vec!(i64);
    len_if_vec!(usize);
    len_if_vec!(u8);
    len_if_vec!(u16);
    len_if_vec!(u32);
    len_if_vec!(u64);
    len_if_vec!(f32);
    len_if_vec!(f64);
    panic_value("reflect: Len of non-slice value");
}

pub fn reflect_value_swapper(value: &dyn Any) -> GorsReflectSwapper {
    let Some(value) = value.downcast_ref::<GorsReflectValue>() else {
        panic_value("reflect: Swapper of non-slice value");
    };
    let value = value.clone();
    Arc::new(Mutex::new(Some(Arc::new(move |i, j| {
        value.swap(i, j);
    }))))
}

pub fn reflect_type_comparable(value: &dyn Any) -> bool {
    if let Some(value) = value.downcast_ref::<Box<dyn Any>>() {
        return reflect_type_comparable(value.as_ref());
    }
    if let Some(value) = value.downcast_ref::<Box<dyn Any + Send>>() {
        return reflect_type_comparable(value.as_ref());
    }
    if let Some(value) = value.downcast_ref::<Box<dyn Any + Send + Sync>>() {
        return reflect_type_comparable(value.as_ref());
    }
    if interface_is_nil(value) {
        return false;
    }
    if value.is::<GorsReflectValue>()
        || value.is::<Vec<u8>>()
        || value.is::<Vec<std::string::String>>()
        || value.is::<Vec<Box<dyn Any>>>()
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
            None => GorsInterfaceKey::nil(),
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

#[inline]
pub fn append<C, E>(v: C, elem: E) -> C
where
    C: Append<E>,
{
    v.append_value(elem)
}

#[inline]
pub fn append_slice<T: Clone>(mut v: Vec<T>, elems: &[T]) -> Vec<T> {
    v.extend_from_slice(elems);
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
    value.type_id() == TypeId::of::<()>()
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
