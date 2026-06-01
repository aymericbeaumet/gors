#![allow(
    dead_code,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals
)]

use std::any::{Any, TypeId};
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

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

pub trait error: Send + Sync {
    fn __gors_as_any(&self) -> Option<&dyn Any>;
    fn Error(&self) -> std::string::String;
}

#[derive(Clone, Default)]
pub struct __GorsNooperror;

impl error for __GorsNooperror {
    fn __gors_as_any(&self) -> Option<&dyn Any> {
        None
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

    fn Error(&self) -> std::string::String {
        self.0.clone()
    }
}

impl Default for Box<dyn error> {
    fn default() -> Self {
        Box::new(__GorsNooperror)
    }
}

impl error for Box<dyn error> {
    fn __gors_as_any(&self) -> Option<&dyn Any> {
        (**self).__gors_as_any()
    }

    fn Error(&self) -> std::string::String {
        (**self).Error()
    }
}

pub fn error_string(value: &mut Box<dyn error>) -> std::string::String {
    if value.__gors_as_any().is_none() {
        std::string::String::new()
    } else {
        value.Error()
    }
}

pub fn clone_any(value: &Box<dyn Any>) -> Box<dyn Any> {
    clone_any_ref(value.as_ref())
}

pub fn clone_any_ref(value: &dyn Any) -> Box<dyn Any> {
    macro_rules! clone_if {
        ($ty:ty) => {
            if let Some(v) = value.downcast_ref::<$ty>() {
                return Box::new(v.clone()) as Box<dyn Any>;
            }
        };
    }

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

    Box::new(())
}

pub const r#true: r#bool = true;
pub const r#false: r#bool = false;
pub const iota: int = 0;
pub const nil: Option<()> = None;

static RECOVER_PAYLOAD: Mutex<Option<Box<dyn Any + Send>>> = Mutex::new(None);

fn recover_payload_lock() -> MutexGuard<'static, Option<Box<dyn Any + Send>>> {
    match RECOVER_PAYLOAD.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

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

impl ByteSeq for std::string::String {
    fn byte_at(&self, index: usize) -> u8 {
        self.as_bytes().get(index).copied().unwrap_or_default()
    }

    fn byte_slice(&self, start: usize, end: usize) -> Vec<u8> {
        self.as_bytes()
            .get(start..end)
            .map_or_else(Vec::new, <[u8]>::to_vec)
    }
}

impl ByteSeq for Vec<u8> {
    fn byte_at(&self, index: usize) -> u8 {
        self.get(index).copied().unwrap_or_default()
    }

    fn byte_slice(&self, start: usize, end: usize) -> Vec<u8> {
        self.get(start..end).map_or_else(Vec::new, <[u8]>::to_vec)
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
    std::string::String::from_utf8(value.byte_slice(0, value.len_value())).unwrap_or_default()
}

impl<T> Len for Vec<T> {
    fn len_value(&self) -> usize {
        self.len()
    }
}

impl Len for std::string::String {
    fn len_value(&self) -> usize {
        self.len()
    }
}

impl Len for str {
    fn len_value(&self) -> usize {
        self.len()
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

impl<K, V> Len for HashMap<K, V> {
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

impl<T, const N: usize> Cap for [T; N] {
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
        self.extend(elem.into_bytes());
        self
    }
}

impl Append<&str> for Vec<u8> {
    fn append_value(mut self, elem: &str) -> Self {
        self.extend(elem.as_bytes());
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
        std::string::String::from_utf8(self).unwrap_or_default()
    }
}

impl StringValue for &Vec<u8> {
    fn string_value(self) -> std::string::String {
        std::string::String::from_utf8(self.clone()).unwrap_or_default()
    }
}

impl StringValue for Vec<i32> {
    fn string_value(self) -> std::string::String {
        self.into_iter()
            .filter_map(|r| char::from_u32(r as u32))
            .collect()
    }
}

impl StringValue for &Vec<i32> {
    fn string_value(self) -> std::string::String {
        self.iter()
            .filter_map(|&r| char::from_u32(r as u32))
            .collect()
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
        std::string::String::from_utf8(self.to_vec()).unwrap_or_default()
    }
}

#[inline]
pub fn string<T: StringValue>(v: T) -> std::string::String {
    v.string_value()
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

#[inline]
pub fn delete<K: Hash + Eq, V>(m: &mut HashMap<K, V>, key: &K) {
    m.remove(key);
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
pub fn make_map<K, V>() -> HashMap<K, V> {
    HashMap::new()
}

#[inline]
pub fn make_map_cap<K, V>(cap: usize) -> HashMap<K, V> {
    HashMap::with_capacity(cap)
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

pub struct Chan<T> {
    inner: Arc<(Mutex<ChanInner<T>>, Condvar, Condvar)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TryRecvError {
    Empty,
}

impl<T> Clone for Chan<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> PartialEq for Chan<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl<T> Eq for Chan<T> {}

impl<T> Default for Chan<T> {
    fn default() -> Self {
        Self::new(0)
    }
}

impl<T> Chan<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new((
                Mutex::new(ChanInner {
                    buf: VecDeque::with_capacity(capacity),
                    capacity,
                    closed: false,
                }),
                Condvar::new(),
                Condvar::new(),
            )),
        }
    }

    #[allow(clippy::significant_drop_tightening)]
    pub fn send(&self, val: T) {
        let (lock, rx_cv, tx_cv) = &*self.inner;
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
        let (lock, rx_cv, tx_cv) = &*self.inner;
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
        let (lock, rx_cv, _) = &*self.inner;
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
        let (lock, _, tx_cv) = &*self.inner;
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
        let (lock, _, tx_cv) = &*self.inner;
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
        let (lock, rx_cv, tx_cv) = &*self.inner;
        let mut inner = lock_chan(lock);
        if inner.closed {
            return;
        }
        inner.closed = true;
        rx_cv.notify_all();
        tx_cv.notify_all();
    }

    pub fn len(&self) -> usize {
        let (lock, _, _) = &*self.inner;
        lock_chan(lock).buf.len()
    }

    pub fn is_empty(&self) -> bool {
        let (lock, _, _) = &*self.inner;
        lock_chan(lock).buf.is_empty()
    }

    pub fn cap(&self) -> usize {
        let (lock, _, _) = &*self.inner;
        lock_chan(lock).capacity
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

#[inline]
pub fn set_recover_payload<T: Any + Send + 'static>(value: T) {
    *recover_payload_lock() = Some(Box::new(value));
}

#[inline]
pub fn set_recover_payload_box(value: Box<dyn Any + Send>) {
    *recover_payload_lock() = Some(value);
}

#[inline]
pub fn recover() -> Box<dyn Any + Send> {
    recover_payload_lock()
        .take()
        .unwrap_or_else(|| Box::new(()))
}

#[inline]
pub fn resume_unrecovered_panic() {
    let payload = recover_payload_lock().take();
    if let Some(payload) = payload {
        std::panic::resume_unwind(payload);
    }
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

pub fn append_float(
    mut dst: Vec<u8>,
    value: f64,
    fmt: u8,
    prec: isize,
    _bit_size: isize,
) -> Vec<u8> {
    let precision = usize::try_from(prec).ok();
    let formatted = match fmt as char {
        'f' => precision.map_or_else(|| format!("{value}"), |p| format!("{value:.p$}")),
        'e' => precision.map_or_else(|| format!("{value:e}"), |p| format!("{value:.p$e}")),
        'E' => precision.map_or_else(|| format!("{value:E}"), |p| format!("{value:.p$E}")),
        'g' | 'G' => {
            if prec < 0 {
                format!("{value}")
            } else {
                precision.map_or_else(|| format!("{value}"), |p| format!("{value:.p$}"))
            }
        }
        _ => format!("{value}"),
    };
    dst.extend_from_slice(formatted.as_bytes());
    dst
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
    fn len_and_cap_cover_sequences_maps_and_channels() {
        let values = vec![1, 2, 3];
        let array = [1, 2, 3, 4];
        let text = "hello".to_string();
        let mut map = HashMap::new();
        map.insert("a", 1);
        let ch: Chan<i32> = make_chan(2);
        ch.send(1);

        assert_eq!(len(&values), 3);
        assert_eq!(cap(&values), values.capacity());
        assert_eq!(len(&array), 4);
        assert_eq!(cap(&array), 4);
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
        let bytes = vec![b'g', b'o', b'r', b's'];

        assert_eq!(byte_at(&text, 1), b'o');
        assert_eq!(byte_at(&bytes, 2), b'r');
        assert_eq!(byte_slice(&text, 1, 3), vec![b'o', b'r']);
        assert_eq!(byte_slice(&bytes, 0, 2), vec![b'g', b'o']);
        assert_eq!(string_from_byte_seq(&text), "gors");
        assert_eq!(string_from_byte_seq(&bytes), "gors");
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
    fn panic_and_recover_helpers_have_defined_behavior() {
        assert!(interface_is_nil(recover().as_ref()));
        set_recover_payload("boom".to_string());
        let recovered = recover();
        assert!(!interface_is_nil(recovered.as_ref()));
        assert!(interface_is_nil(recover().as_ref()));
        assert_eq!(recover_func(|| {}).as_deref(), None);
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        assert_eq!(
            recover_func(|| panic_value("boom")).as_deref(),
            Some("boom")
        );
        std::panic::set_hook(previous_hook);
    }
}
