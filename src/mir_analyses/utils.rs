// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Various helper functions for working with `mir::Place`.

use log::trace;
use rustc_index::vec::Idx;
use rustc::mir;
use rustc::ty::{self, TyCtxt};
use std::collections::HashSet;
use log::debug;

/// Check if the place `potential_prefix` is a prefix of `place`. For example:
///
/// +   `is_prefix(x.f, x.f) == true`
/// +   `is_prefix(x.f.g, x.f) == true`
/// +   `is_prefix(x.f, x.f.g) == false`
pub fn is_prefix(place: &mir::Place, potential_prefix: &mir::Place) -> bool {
    if place.local == potential_prefix.local
        && potential_prefix.projection.len() <= place.projection.len()
        && &potential_prefix.projection[..] == &place.projection[..potential_prefix.projection.len()] {
        // debug!("is_prefix called on {:#?} and {:#?}, result: true", place, potential_prefix);
        true
    }
    else{
        // debug!("is_prefix called on {:#?} and {:#?}, result: false", place, potential_prefix);
        false
    }
}

pub fn predecessors_for(mir: &mir::Body, target_bb: mir::BasicBlock) -> Vec<mir::BasicBlock> {
    let mut pred_bb: Vec<mir::BasicBlock> = vec![];
    for (bb, bb_data) in mir.basic_blocks().iter_enumerated() {
        if let Some(ref term) = bb_data.terminator {
            if term.successors().any(|succ_bb| *succ_bb == target_bb) {
                pred_bb.push(bb);
            }
        }
    }
    pred_bb
}

/// Expands a place `x.f.g` of type struct into a vector of places for
/// each of the struct's fields `{x.f.g.f, x.f.g.g, x.f.g.h}`. If
/// `without_field` is not `None`, then omits that field from the final
/// vector.
pub fn expand_struct_place<'a, 'tcx: 'a>(
    place: &mir::Place<'tcx>,
    mir: &mir::Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    without_element: Option<usize>,
) -> Vec<mir::Place<'tcx>> {
    let mut places: Vec<mir::Place<'tcx>> = Vec::new();
    unimplemented!();
    // match place.ty(mir, tcx) {
    //     mir::tcx::PlaceTy::Ty { ty: base_ty } => match base_ty.sty {
    //         ty::Adt(def, substs) => {
    //             assert!(
    //                 def.variants.len() == 1,
    //                 "Only structs can be expanded. Got def={:?}.",
    //                 def
    //             );
    //             let variant_0 = ty::layout::VariantIdx::from_usize(0);
    //             for (index, field_def) in def.variants[variant_0].fields.iter().enumerate() {
    //                 if Some(index) != without_element {
    //                     let field = mir::Field::new(index);
    //                     places.push(place.clone().field(field, field_def.ty(tcx, substs)));
    //                 }
    //             }
    //         }
    //         ty::Tuple(slice) => for (index, ty) in slice.iter().enumerate() {
    //             if Some(index) != without_element {
    //                 let field = mir::Field::new(index);
    //                 places.push(place.clone().field(field, ty));
    //             }
    //         },
    //         ty::Ref(_region, _ty, _) => {
    //             match without_element {
    //                 Some(without_element) => {
    //                     assert_eq!(
    //                         without_element,
    //                         0,
    //                         "References have only a single “field”.");
    //                 }
    //                 None => {
    //                     places.push(place.clone().deref());
    //                 }
    //             }
    //         },
    //         ref ty => {
    //             unimplemented!("ty={:?}", ty);
    //         },
    //     },
    //     mir::tcx::PlaceTy::Downcast { .. } => {}
    // }
    // places
}

/// Subtract the `subtrahend` place from the `minuend` place. The
/// subtraction is defined as set minus between `minuend` place replaced
/// with a set of places that are unrolled up to the same level as
/// `subtrahend` and the singleton `subtrahend` set. For example,
/// `subtract(x.f, x.f.g.h)` is performed by unrolling `x.f` into
/// `{x.g, x.h, x.f.f, x.f.h, x.f.g.f, x.f.g.g, x.f.g.h}` and
/// subtracting `{x.f.g.h}` from it, which results into `{x.g, x.h,
/// x.f.f, x.f.h, x.f.g.f, x.f.g.g}`.
pub fn expand<'a, 'tcx: 'a>(
    mir: &mir::Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    minuend: &mir::Place<'tcx>,
    subtrahend: &mir::Place<'tcx>,
) -> Vec<mir::Place<'tcx>> {
    assert!(
        is_prefix(subtrahend, minuend),
        "The minuend must be the prefix of the subtrahend."
    );
    trace!(
        "[enter] expand minuend={:?} subtrahend={:?}",
        minuend,
        subtrahend
    );
    let mut place_set = Vec::new();
    fn expand_recursively<'a, 'tcx: 'a>(
        place_set: &mut Vec<mir::Place<'tcx>>,
        mir: &mir::Body<'tcx>,
        tcx: TyCtxt<'tcx>,
        minuend: &mir::Place<'tcx>,
        subtrahend: &mir::Place<'tcx>,
    ) {
        trace!(
            "[enter] expand_recursively minuend={:?} subtrahend={:?}",
            minuend,
            subtrahend
        );
        // if minuend != subtrahend {
        //     match subtrahend {
        //         mir::Place::Projection([base, elem]) => {
        //             // We just recurse until both paths become equal.
        //             expand_recursively(place_set, mir, tcx, minuend, base);
        //             match elem {
        //                 mir::ProjectionElem::Field(projected_field, _field_ty) => {
        //                     let places =
        //                         expand_struct_place(base, mir, tcx, Some(projected_field.index()));
        //                     place_set.extend(places);
        //                 }
        //                 mir::ProjectionElem::Downcast(_def, _variant) => {}
        //                 mir::ProjectionElem::Deref => {}
        //                 elem => {
        //                     unimplemented!("elem = {:?}", elem);
        //                 }
        //             }
        //         }
        //         _ => unreachable!(),
        //     }
        // }
    };
    expand_recursively(&mut place_set, mir, tcx, minuend, subtrahend);
    trace!(
        "[exit] expand minuend={:?} subtrahend={:?} place_set={:?}",
        minuend,
        subtrahend,
        place_set
    );
    place_set
}

/// Try to collapse all places in `places` by following the
/// `guide_place`. This function is basically the reverse of
/// `expand_struct_place`.
pub fn collapse<'a, 'tcx: 'a>(
    mir: &mir::Body<'tcx>,
    tcx: TyCtxt<'tcx>,
    places: &mut HashSet<mir::Place<'tcx>>,
    guide_place: &mir::Place<'tcx>,
) {
    // let mut guide_place = guide_place.clone();
    // while let mir::Place::Projection([base, ..]) = guide_place {
    //     let expansion = expand_struct_place(&base, mir, tcx, None);
    //     if expansion.iter().all(|place| places.contains(place)) {
    //         for place in expansion {
    //             places.remove(&place);
    //         }
    //         places.insert(base.clone());
    //     } else {
    //         return;
    //     }
    //     guide_place = base;
    // }
}
