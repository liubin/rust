use super::OverlapError;

use crate::traits;
use rustc::ty::fast_reject::{self, SimplifiedType};
use rustc::ty::{self, TyCtxt, TypeFoldable};
use rustc_hir::def_id::DefId;

pub use rustc::traits::types::specialization_graph::*;

#[derive(Copy, Clone, Debug)]
pub enum FutureCompatOverlapErrorKind {
    Issue43355,
    Issue33140,
    LeakCheck,
}

#[derive(Debug)]
pub struct FutureCompatOverlapError {
    pub error: OverlapError,
    pub kind: FutureCompatOverlapErrorKind,
}

/// The result of attempting to insert an impl into a group of children.
enum Inserted {
    /// The impl was inserted as a new child in this group of children.
    BecameNewSibling(Option<FutureCompatOverlapError>),

    /// The impl should replace existing impls [X1, ..], because the impl specializes X1, X2, etc.
    ReplaceChildren(Vec<DefId>),

    /// The impl is a specialization of an existing child.
    ShouldRecurseOn(DefId),
}

impl<'tcx> Children {
    /// Insert an impl into this set of children without comparing to any existing impls.
    fn insert_blindly(&mut self, tcx: TyCtxt<'tcx>, impl_def_id: DefId) {
        let trait_ref = tcx.impl_trait_ref(impl_def_id).unwrap();
        if let Some(st) = fast_reject::simplify_type(tcx, trait_ref.self_ty(), false) {
            debug!("insert_blindly: impl_def_id={:?} st={:?}", impl_def_id, st);
            self.nonblanket_impls.entry(st).or_default().push(impl_def_id)
        } else {
            debug!("insert_blindly: impl_def_id={:?} st=None", impl_def_id);
            self.blanket_impls.push(impl_def_id)
        }
    }

    /// Removes an impl from this set of children. Used when replacing
    /// an impl with a parent. The impl must be present in the list of
    /// children already.
    fn remove_existing(&mut self, tcx: TyCtxt<'tcx>, impl_def_id: DefId) {
        let trait_ref = tcx.impl_trait_ref(impl_def_id).unwrap();
        let vec: &mut Vec<DefId>;
        if let Some(st) = fast_reject::simplify_type(tcx, trait_ref.self_ty(), false) {
            debug!("remove_existing: impl_def_id={:?} st={:?}", impl_def_id, st);
            vec = self.nonblanket_impls.get_mut(&st).unwrap();
        } else {
            debug!("remove_existing: impl_def_id={:?} st=None", impl_def_id);
            vec = &mut self.blanket_impls;
        }

        let index = vec.iter().position(|d| *d == impl_def_id).unwrap();
        vec.remove(index);
    }

    /// Attempt to insert an impl into this set of children, while comparing for
    /// specialization relationships.
    fn insert(
        &mut self,
        tcx: TyCtxt<'tcx>,
        impl_def_id: DefId,
        simplified_self: Option<SimplifiedType>,
    ) -> Result<Inserted, OverlapError> {
        let mut last_lint = None;
        let mut replace_children = Vec::new();

        debug!("insert(impl_def_id={:?}, simplified_self={:?})", impl_def_id, simplified_self,);

        let possible_siblings = match simplified_self {
            Some(st) => PotentialSiblings::Filtered(self.filtered(st)),
            None => PotentialSiblings::Unfiltered(self.iter()),
        };

        for possible_sibling in possible_siblings {
            debug!(
                "insert: impl_def_id={:?}, simplified_self={:?}, possible_sibling={:?}",
                impl_def_id, simplified_self, possible_sibling,
            );

            let overlap_error = |overlap: traits::coherence::OverlapResult<'_>| {
                // Found overlap, but no specialization; error out.
                let trait_ref = overlap.impl_header.trait_ref.unwrap();
                let self_ty = trait_ref.self_ty();
                OverlapError {
                    with_impl: possible_sibling,
                    trait_desc: trait_ref.print_only_trait_path().to_string(),
                    // Only report the `Self` type if it has at least
                    // some outer concrete shell; otherwise, it's
                    // not adding much information.
                    self_desc: if self_ty.has_concrete_skeleton() {
                        Some(self_ty.to_string())
                    } else {
                        None
                    },
                    intercrate_ambiguity_causes: overlap.intercrate_ambiguity_causes,
                    involves_placeholder: overlap.involves_placeholder,
                }
            };

            let (le, ge) = traits::overlapping_impls(
                tcx,
                possible_sibling,
                impl_def_id,
                traits::IntercrateMode::Issue43355,
                traits::SkipLeakCheck::default(),
                |overlap| {
                    if let Some(overlap_kind) =
                        tcx.impls_are_allowed_to_overlap(impl_def_id, possible_sibling)
                    {
                        match overlap_kind {
                            ty::ImplOverlapKind::Permitted { marker: _ } => {}
                            ty::ImplOverlapKind::Issue33140 => {
                                last_lint = Some(FutureCompatOverlapError {
                                    error: overlap_error(overlap),
                                    kind: FutureCompatOverlapErrorKind::Issue33140,
                                });
                            }
                        }

                        return Ok((false, false));
                    }

                    let le = tcx.specializes((impl_def_id, possible_sibling));
                    let ge = tcx.specializes((possible_sibling, impl_def_id));

                    if le == ge { Err(overlap_error(overlap)) } else { Ok((le, ge)) }
                },
                || Ok((false, false)),
            )?;

            if le && !ge {
                debug!(
                    "descending as child of TraitRef {:?}",
                    tcx.impl_trait_ref(possible_sibling).unwrap()
                );

                // The impl specializes `possible_sibling`.
                return Ok(Inserted::ShouldRecurseOn(possible_sibling));
            } else if ge && !le {
                debug!(
                    "placing as parent of TraitRef {:?}",
                    tcx.impl_trait_ref(possible_sibling).unwrap()
                );

                replace_children.push(possible_sibling);
            } else {
                if let None = tcx.impls_are_allowed_to_overlap(impl_def_id, possible_sibling) {
                    // do future-compat checks for overlap. Have issue #33140
                    // errors overwrite issue #43355 errors when both are present.

                    traits::overlapping_impls(
                        tcx,
                        possible_sibling,
                        impl_def_id,
                        traits::IntercrateMode::Fixed,
                        traits::SkipLeakCheck::default(),
                        |overlap| {
                            last_lint = Some(FutureCompatOverlapError {
                                error: overlap_error(overlap),
                                kind: FutureCompatOverlapErrorKind::Issue43355,
                            });
                        },
                        || (),
                    );

                    if last_lint.is_none() {
                        traits::overlapping_impls(
                            tcx,
                            possible_sibling,
                            impl_def_id,
                            traits::IntercrateMode::Fixed,
                            traits::SkipLeakCheck::Yes,
                            |overlap| {
                                last_lint = Some(FutureCompatOverlapError {
                                    error: overlap_error(overlap),
                                    kind: FutureCompatOverlapErrorKind::LeakCheck,
                                });
                            },
                            || (),
                        );
                    }
                }

                // no overlap (error bailed already via ?)
            }
        }

        if !replace_children.is_empty() {
            return Ok(Inserted::ReplaceChildren(replace_children));
        }

        // No overlap with any potential siblings, so add as a new sibling.
        debug!("placing as new sibling");
        self.insert_blindly(tcx, impl_def_id);
        Ok(Inserted::BecameNewSibling(last_lint))
    }

    fn iter(&mut self) -> impl Iterator<Item = DefId> + '_ {
        let nonblanket = self.nonblanket_impls.iter_mut().flat_map(|(_, v)| v.iter());
        self.blanket_impls.iter().chain(nonblanket).cloned()
    }

    fn filtered(&mut self, st: SimplifiedType) -> impl Iterator<Item = DefId> + '_ {
        let nonblanket = self.nonblanket_impls.entry(st).or_default().iter();
        self.blanket_impls.iter().chain(nonblanket).cloned()
    }
}

// A custom iterator used by Children::insert
enum PotentialSiblings<I, J>
where
    I: Iterator<Item = DefId>,
    J: Iterator<Item = DefId>,
{
    Unfiltered(I),
    Filtered(J),
}

impl<I, J> Iterator for PotentialSiblings<I, J>
where
    I: Iterator<Item = DefId>,
    J: Iterator<Item = DefId>,
{
    type Item = DefId;

    fn next(&mut self) -> Option<Self::Item> {
        match *self {
            PotentialSiblings::Unfiltered(ref mut iter) => iter.next(),
            PotentialSiblings::Filtered(ref mut iter) => iter.next(),
        }
    }
}

impl<'tcx> Graph {
    /// Insert a local impl into the specialization graph. If an existing impl
    /// conflicts with it (has overlap, but neither specializes the other),
    /// information about the area of overlap is returned in the `Err`.
    pub fn insert(
        &mut self,
        tcx: TyCtxt<'tcx>,
        impl_def_id: DefId,
    ) -> Result<Option<FutureCompatOverlapError>, OverlapError> {
        assert!(impl_def_id.is_local());

        let trait_ref = tcx.impl_trait_ref(impl_def_id).unwrap();
        let trait_def_id = trait_ref.def_id;

        debug!(
            "insert({:?}): inserting TraitRef {:?} into specialization graph",
            impl_def_id, trait_ref
        );

        // If the reference itself contains an earlier error (e.g., due to a
        // resolution failure), then we just insert the impl at the top level of
        // the graph and claim that there's no overlap (in order to suppress
        // bogus errors).
        if trait_ref.references_error() {
            debug!(
                "insert: inserting dummy node for erroneous TraitRef {:?}, \
                 impl_def_id={:?}, trait_def_id={:?}",
                trait_ref, impl_def_id, trait_def_id
            );

            self.parent.insert(impl_def_id, trait_def_id);
            self.children.entry(trait_def_id).or_default().insert_blindly(tcx, impl_def_id);
            return Ok(None);
        }

        let mut parent = trait_def_id;
        let mut last_lint = None;
        let simplified = fast_reject::simplify_type(tcx, trait_ref.self_ty(), false);

        // Descend the specialization tree, where `parent` is the current parent node.
        loop {
            use self::Inserted::*;

            let insert_result =
                self.children.entry(parent).or_default().insert(tcx, impl_def_id, simplified)?;

            match insert_result {
                BecameNewSibling(opt_lint) => {
                    last_lint = opt_lint;
                    break;
                }
                ReplaceChildren(grand_children_to_be) => {
                    // We currently have
                    //
                    //     P
                    //     |
                    //     G
                    //
                    // and we are inserting the impl N. We want to make it:
                    //
                    //     P
                    //     |
                    //     N
                    //     |
                    //     G

                    // Adjust P's list of children: remove G and then add N.
                    {
                        let siblings = self.children.get_mut(&parent).unwrap();
                        for &grand_child_to_be in &grand_children_to_be {
                            siblings.remove_existing(tcx, grand_child_to_be);
                        }
                        siblings.insert_blindly(tcx, impl_def_id);
                    }

                    // Set G's parent to N and N's parent to P.
                    for &grand_child_to_be in &grand_children_to_be {
                        self.parent.insert(grand_child_to_be, impl_def_id);
                    }
                    self.parent.insert(impl_def_id, parent);

                    // Add G as N's child.
                    for &grand_child_to_be in &grand_children_to_be {
                        self.children
                            .entry(impl_def_id)
                            .or_default()
                            .insert_blindly(tcx, grand_child_to_be);
                    }
                    break;
                }
                ShouldRecurseOn(new_parent) => {
                    parent = new_parent;
                }
            }
        }

        self.parent.insert(impl_def_id, parent);
        Ok(last_lint)
    }

    /// Insert cached metadata mapping from a child impl back to its parent.
    pub fn record_impl_from_cstore(&mut self, tcx: TyCtxt<'tcx>, parent: DefId, child: DefId) {
        if self.parent.insert(child, parent).is_some() {
            bug!(
                "When recording an impl from the crate store, information about its parent \
                 was already present."
            );
        }

        self.children.entry(parent).or_default().insert_blindly(tcx, child);
    }
}
