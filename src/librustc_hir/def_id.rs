use rustc_data_structures::fingerprint::Fingerprint;
use rustc_data_structures::AtomicRef;
use rustc_index::vec::Idx;
use rustc_macros::HashStable_Generic;
use rustc_serialize::{Decoder, Encoder};
use std::borrow::Borrow;
use std::fmt;
use std::{u32, u64};

rustc_index::newtype_index! {
    pub struct CrateId {
        ENCODABLE = custom
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CrateNum {
    /// A special `CrateNum` that we use for the `tcx.rcache` when decoding from
    /// the incr. comp. cache.
    ReservedForIncrCompCache,
    Index(CrateId),
}

impl ::std::fmt::Debug for CrateNum {
    fn fmt(&self, fmt: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match self {
            CrateNum::Index(id) => write!(fmt, "crate{}", id.private),
            CrateNum::ReservedForIncrCompCache => write!(fmt, "crate for decoding incr comp cache"),
        }
    }
}

/// Item definitions in the currently-compiled crate would have the `CrateNum`
/// `LOCAL_CRATE` in their `DefId`.
pub const LOCAL_CRATE: CrateNum = CrateNum::Index(CrateId::from_u32_const(0));

impl Idx for CrateNum {
    #[inline]
    fn new(value: usize) -> Self {
        CrateNum::Index(Idx::new(value))
    }

    #[inline]
    fn index(self) -> usize {
        match self {
            CrateNum::Index(idx) => Idx::index(idx),
            _ => panic!("Tried to get crate index of {:?}", self),
        }
    }
}

impl CrateNum {
    pub fn new(x: usize) -> CrateNum {
        CrateNum::from_usize(x)
    }

    pub fn from_usize(x: usize) -> CrateNum {
        CrateNum::Index(CrateId::from_usize(x))
    }

    pub fn from_u32(x: u32) -> CrateNum {
        CrateNum::Index(CrateId::from_u32(x))
    }

    pub fn as_usize(self) -> usize {
        match self {
            CrateNum::Index(id) => id.as_usize(),
            _ => panic!("tried to get index of non-standard crate {:?}", self),
        }
    }

    pub fn as_u32(self) -> u32 {
        match self {
            CrateNum::Index(id) => id.as_u32(),
            _ => panic!("tried to get index of non-standard crate {:?}", self),
        }
    }

    pub fn as_def_id(&self) -> DefId {
        DefId { krate: *self, index: CRATE_DEF_INDEX }
    }
}

impl fmt::Display for CrateNum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CrateNum::Index(id) => fmt::Display::fmt(&id.private, f),
            CrateNum::ReservedForIncrCompCache => write!(f, "crate for decoding incr comp cache"),
        }
    }
}

/// As a local identifier, a `CrateNum` is only meaningful within its context, e.g. within a tcx.
/// Therefore, make sure to include the context when encode a `CrateNum`.
impl rustc_serialize::UseSpecializedEncodable for CrateNum {
    fn default_encode<E: Encoder>(&self, e: &mut E) -> Result<(), E::Error> {
        e.emit_u32(self.as_u32())
    }
}
impl rustc_serialize::UseSpecializedDecodable for CrateNum {
    fn default_decode<D: Decoder>(d: &mut D) -> Result<CrateNum, D::Error> {
        Ok(CrateNum::from_u32(d.read_u32()?))
    }
}

#[derive(
    Copy,
    Clone,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Debug,
    RustcEncodable,
    RustcDecodable,
    HashStable_Generic
)]
pub struct DefPathHash(pub Fingerprint);

impl Borrow<Fingerprint> for DefPathHash {
    #[inline]
    fn borrow(&self) -> &Fingerprint {
        &self.0
    }
}

rustc_index::newtype_index! {
    /// A DefIndex is an index into the hir-map for a crate, identifying a
    /// particular definition. It should really be considered an interned
    /// shorthand for a particular DefPath.
    pub struct DefIndex {
        DEBUG_FORMAT = "DefIndex({})",

        /// The crate root is always assigned index 0 by the AST Map code,
        /// thanks to `NodeCollector::new`.
        const CRATE_DEF_INDEX = 0,
    }
}

impl rustc_serialize::UseSpecializedEncodable for DefIndex {}
impl rustc_serialize::UseSpecializedDecodable for DefIndex {}

/// A `DefId` identifies a particular *definition*, by combining a crate
/// index and a def index.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Copy)]
pub struct DefId {
    pub krate: CrateNum,
    pub index: DefIndex,
}

impl DefId {
    /// Makes a local `DefId` from the given `DefIndex`.
    #[inline]
    pub fn local(index: DefIndex) -> DefId {
        DefId { krate: LOCAL_CRATE, index: index }
    }

    #[inline]
    pub fn is_local(self) -> bool {
        self.krate == LOCAL_CRATE
    }

    #[inline]
    pub fn to_local(self) -> LocalDefId {
        LocalDefId::from_def_id(self)
    }

    pub fn is_top_level_module(self) -> bool {
        self.is_local() && self.index == CRATE_DEF_INDEX
    }
}

impl rustc_serialize::UseSpecializedEncodable for DefId {
    fn default_encode<S: Encoder>(&self, s: &mut S) -> Result<(), S::Error> {
        let krate = u64::from(self.krate.as_u32());
        let index = u64::from(self.index.as_u32());
        s.emit_u64((krate << 32) | index)
    }
}
impl rustc_serialize::UseSpecializedDecodable for DefId {
    fn default_decode<D: Decoder>(d: &mut D) -> Result<DefId, D::Error> {
        let def_id = d.read_u64()?;
        let krate = CrateNum::from_u32((def_id >> 32) as u32);
        let index = DefIndex::from_u32((def_id & 0xffffffff) as u32);
        Ok(DefId { krate, index })
    }
}

pub fn default_def_id_debug(def_id: DefId, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("DefId").field("krate", &def_id.krate).field("index", &def_id.index).finish()
}

pub static DEF_ID_DEBUG: AtomicRef<fn(DefId, &mut fmt::Formatter<'_>) -> fmt::Result> =
    AtomicRef::new(&(default_def_id_debug as fn(_, &mut fmt::Formatter<'_>) -> _));

impl fmt::Debug for DefId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (*DEF_ID_DEBUG)(*self, f)
    }
}

rustc_data_structures::define_id_collections!(DefIdMap, DefIdSet, DefId);

/// A LocalDefId is equivalent to a DefId with `krate == LOCAL_CRATE`. Since
/// we encode this information in the type, we can ensure at compile time that
/// no DefIds from upstream crates get thrown into the mix. There are quite a
/// few cases where we know that only DefIds from the local crate are expected
/// and a DefId from a different crate would signify a bug somewhere. This
/// is when LocalDefId comes in handy.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalDefId(DefIndex);

impl LocalDefId {
    #[inline]
    pub fn from_def_id(def_id: DefId) -> LocalDefId {
        assert!(def_id.is_local());
        LocalDefId(def_id.index)
    }

    #[inline]
    pub fn to_def_id(self) -> DefId {
        DefId { krate: LOCAL_CRATE, index: self.0 }
    }
}

impl fmt::Debug for LocalDefId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.to_def_id().fmt(f)
    }
}

impl rustc_serialize::UseSpecializedEncodable for LocalDefId {}
impl rustc_serialize::UseSpecializedDecodable for LocalDefId {}
