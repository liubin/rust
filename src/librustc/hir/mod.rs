//! HIR datatypes. See the [rustc guide] for more info.
//!
//! [rustc guide]: https://rust-lang.github.io/rustc-guide/hir.html

pub mod check_attr;
pub mod exports;
pub mod map;

use crate::ty::query::Providers;
use crate::ty::TyCtxt;
use rustc_data_structures::fx::FxHashMap;
use rustc_hir::def_id::DefId;
use rustc_hir::def_id::LOCAL_CRATE;
use rustc_hir::print;
use rustc_hir::Body;
use rustc_hir::BodyId;
use rustc_hir::Crate;
use rustc_hir::HirId;
use rustc_hir::ItemLocalId;
use rustc_hir::Node;
use rustc_index::vec::IndexVec;
use std::ops::Deref;

#[derive(HashStable)]
pub struct HirOwner<'tcx> {
    parent: HirId,
    node: Node<'tcx>,
}

#[derive(HashStable, Clone)]
pub struct HirItem<'tcx> {
    parent: ItemLocalId,
    node: Node<'tcx>,
}

#[derive(HashStable)]
pub struct HirOwnerItems<'tcx> {
    //owner: &'tcx HirOwner<'tcx>,
    items: IndexVec<ItemLocalId, Option<HirItem<'tcx>>>,
    bodies: FxHashMap<ItemLocalId, &'tcx Body<'tcx>>,
}

/// A wrapper type which allows you to access HIR.
#[derive(Clone)]
pub struct Hir<'tcx> {
    tcx: TyCtxt<'tcx>,
    map: &'tcx map::Map<'tcx>,
}

impl<'tcx> Hir<'tcx> {
    pub fn krate(&self) -> &'tcx Crate<'tcx> {
        self.tcx.hir_crate(LOCAL_CRATE)
    }

    pub fn body(&self, id: BodyId) -> &'tcx Body<'tcx> {
        self.tcx
            .hir_owner_items(DefId::local(id.hir_id.owner))
            .bodies
            .get(&id.hir_id.local_id)
            .unwrap()
    }
}

impl<'tcx> Deref for Hir<'tcx> {
    type Target = &'tcx map::Map<'tcx>;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl<'hir> print::PpAnn for Hir<'hir> {
    fn nested(&self, state: &mut print::State<'_>, nested: print::Nested) {
        self.map.nested(state, nested)
    }
}

impl<'tcx> TyCtxt<'tcx> {
    #[inline(always)]
    pub fn hir(self) -> Hir<'tcx> {
        Hir { tcx: self, map: &self.hir_map }
    }
}

pub fn provide(providers: &mut Providers<'_>) {
    providers.hir_crate = |tcx, _| tcx.hir_map.untracked_krate();
    providers.hir_owner = |tcx, id| {
        assert_eq!(id.krate, LOCAL_CRATE);
        *tcx.hir_map.owner_map.get(&id.index).unwrap()
    };
    providers.hir_owner_items = |tcx, id| {
        assert_eq!(id.krate, LOCAL_CRATE);
        *tcx.hir_map.owner_items_map.get(&id.index).unwrap()
    };
    map::provide(providers);
}
