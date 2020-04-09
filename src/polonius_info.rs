// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use log::debug;
use rustc_hir::def_id::DefId;
use rustc::mir;
use rustc::ty;
use std::collections::HashMap;
use super::borrowck::{facts, regions};
use polonius_engine::{Algorithm, Output, Atom};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct LoanPlaces<'tcx> {
    pub dest: mir::Place<'tcx>,
    pub source: mir::Rvalue<'tcx>,
    pub location: mir::Location,
}

pub struct PoloniusInfo {
    pub(crate) borrowck_in_facts: facts::AllInputFacts,
    pub(crate) borrowck_out_facts: facts::AllOutputFacts,
    pub(crate) interner: facts::Interner,
    pub variable_regions: HashMap<mir::Local, facts::Region>,
}

/// Returns moves and argument moves that were turned into fake reborrows.
fn add_fake_facts<'a, 'tcx:'a>(
    all_facts: &mut facts::AllInputFacts,
    interner: &facts::Interner,
    mir: &'a mir::Body<'tcx>,
    variable_regions: &HashMap<mir::Local, facts::Region>,
    call_magic_wands: &mut HashMap<facts::Loan, mir::Local>
) -> (Vec<facts::Loan>, Vec<facts::Loan>) {
    // The code that adds a creation of a new borrow for each
    // move of a borrow.

    let mut reference_moves = Vec::new();
    let mut argument_moves = Vec::new();

    // Find the last loan index.
    let mut last_loan_id = 0;
    for (_, loan, _) in all_facts.borrow_region.iter() {
        if loan.index() > last_loan_id {
            last_loan_id = loan.index();
        }
    }
    last_loan_id += 1;

    // Create a map from points to (region1, region2) vectors.
    let universal_region = &all_facts.universal_region;
    let mut outlives_at_point = HashMap::new();
    for (region1, region2, point) in all_facts.outlives.iter() {
        if !universal_region.contains(region1) && !universal_region.contains(region2) {
            let outlives = outlives_at_point.entry(point).or_insert(vec![]);
            outlives.push((region1, region2));
        }
    }

    // Create new borrow_region facts for points where is only one outlives
    // fact and there is not a borrow_region fact already.
    let borrow_region = &mut all_facts.borrow_region;
    for (point, mut regions) in outlives_at_point {
        if borrow_region.iter().all(|(_, _, loan_point)| loan_point != point) {
            let location = interner.get_point(*point).location.clone();
            if is_call(&mir, location) {
                let call_destination = get_call_destination(&mir, location);
                if let Some(place) = call_destination {
                    debug!("Adding for call destination:");
                    for &(region1, region2) in regions.iter() {
                        debug!("{:?} {:?} {:?}", location, region1, region2);
                    }
                    // match place {
                    //     mir::Place::Local(local) => {
                    //         if let Some(var_region) = variable_regions.get(&local) {
                    //             debug!("var_region = {:?} loan = {}", var_region, last_loan_id);
                    //             let loan = facts::Loan::from(last_loan_id);
                    //             borrow_region.push(
                    //                 (*var_region,
                    //                  loan,
                    //                  *point));
                    //             last_loan_id += 1;
                    //             call_magic_wands.insert(loan, local);
                    //         }
                    //     }
                    //     x => unimplemented!("{:?}", x)
                    // }
                }
                for &(region1, _region2) in &regions {
                    let new_loan = facts::Loan::from(last_loan_id);
                    borrow_region.push((*region1, new_loan, *point));
                    argument_moves.push(new_loan);
                    debug!("Adding call arg: {:?} {:?} {:?} {}",
                           region1, _region2, location, last_loan_id);
                    last_loan_id += 1;
                }
            } else if is_assignment(&mir, location) {
                let (_region1, region2) = regions.pop().unwrap();
                let new_loan = facts::Loan::from(last_loan_id);
                borrow_region.push((*region2, new_loan, *point));
                reference_moves.push(new_loan);
                debug!("Adding generic: {:?} {:?} {:?} {}", _region1, region2, location, last_loan_id);
                last_loan_id += 1;
            }
        }
    }
    (reference_moves, argument_moves)
}

impl PoloniusInfo {
    pub fn new<'a, 'tcx: 'a>(tcx: ty::TyCtxt<'tcx>, def_id: DefId, mir: &'a mir::Body<'tcx>) -> Self {
        // Read Polonius facts.
        let def_path = tcx.hir().def_path(def_id);
        let dir_path = PathBuf::from("nll-facts").join(def_path.to_filename_friendly_no_crate());
        debug!("Reading facts from: {:?}", dir_path);
        let mut facts_loader = facts::FactLoader::new();
        facts_loader.load_all_facts(&dir_path);

        // Read relations between region IDs and local variables.
        let renumber_path = PathBuf::from(format!(
            "log/mir/rustc.{}.-------.renumber.0.mir",
            def_path.to_filename_friendly_no_crate()));
        debug!("Renumber path: {:?}", renumber_path);
        let variable_regions = regions::load_variable_regions(&renumber_path).unwrap();

        //let mir = tcx.mir_validated(def_id).borrow();

        let mut call_magic_wands = HashMap::new();

        let mut all_facts = facts_loader.facts;
        let (_reference_moves, _argument_moves) = add_fake_facts(
            &mut all_facts, &facts_loader.interner, &mir,
            &variable_regions, &mut call_magic_wands);

        let output = Output::compute(&all_facts, Algorithm::Naive, true);

        let interner = facts_loader.interner;

        let info = Self {
            borrowck_in_facts: all_facts,
            borrowck_out_facts: output,
            interner: interner,
            variable_regions: variable_regions,
        };
        info
    }

    /// Find a variable that has the given region in its type.
    pub fn find_variable(&self, region: facts::Region) -> Option<mir::Local> {
        let mut local = None;
        for (key, value) in self.variable_regions.iter() {
            if *value == region {
                assert!(local.is_none());
                local = Some(*key);
            }
        }
        local
    }

}

/// Check if the statement is assignment.
fn is_assignment(mir: &mir::Body,
                 location: mir::Location) -> bool {
    let mir::BasicBlockData { ref statements, .. } = mir[location.block];
    if statements.len() == location.statement_index {
        return false;
    }
    match statements[location.statement_index].kind {
        mir::StatementKind::Assign { .. } => true,
        _ => false,
    }
}

fn is_call(mir: &mir::Body,
           location: mir::Location) -> bool {
    let mir::BasicBlockData { ref statements, ref terminator, .. } = mir[location.block];
    if statements.len() != location.statement_index {
        return false;
    }
    match terminator.as_ref().unwrap().kind {
        mir::TerminatorKind::Call { .. } => true,
        _ => false,
    }
}

/// Extract the call terminator at the location. Otherwise return None.
fn get_call_destination<'tcx>(mir: &mir::Body<'tcx>, location: mir::Location) -> Option<mir::Place<'tcx>> {
    let mir::BasicBlockData { ref statements, ref terminator, .. } = mir[location.block];
    if statements.len() != location.statement_index {
        return None;
    }
    match terminator.as_ref().unwrap().kind {
        mir::TerminatorKind::Call { ref destination, .. } => {
            if let Some((ref place, _)) = destination {
                Some(place.clone())
            } else {
                None
            }
        }
        ref x => {
            panic!("Expected call, got {:?} at {:?}", x, location);
        }
    }
}
