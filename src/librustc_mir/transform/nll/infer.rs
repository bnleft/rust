use super::{Region, RegionIndex};
use std::mem;
use rustc::infer::InferCtxt;
use rustc::mir::{Location, Mir};
use rustc_data_structures::indexed_vec::{Idx, IndexVec};
use rustc_data_structures::fx::FxHashSet;

pub struct InferenceContext {
    definitions: IndexVec<RegionIndex, VarDefinition>,
    constraints: IndexVec<ConstraintIndex, Constraint>,
    errors: IndexVec<InferenceErrorIndex, InferenceError>,
}

pub struct InferenceError {
    pub constraint_point: Location,
    pub name: (), // TODO(nashenas88) RegionName
}

newtype_index!(InferenceErrorIndex);

struct VarDefinition {
    name: (), // TODO(nashenas88) RegionName
    value: Region,
    capped: bool,
}

impl VarDefinition {
    pub fn new(value: Region) -> Self {
        Self {
            name: (),
            value,
            capped: false,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Constraint {
    sub: RegionIndex,
    sup: RegionIndex,
    point: Location,
}

newtype_index!(ConstraintIndex);

impl InferenceContext {
    pub fn new(values: IndexVec<RegionIndex, Region>) -> Self {
        Self {
            definitions: values.into_iter().map(VarDefinition::new).collect(),
            constraints: IndexVec::new(),
            errors: IndexVec::new(),
        }
    }

    pub fn cap_var(&mut self, v: RegionIndex) {
        self.definitions[v].capped = true;
    }

    pub fn add_live_point(&mut self, v: RegionIndex, point: Location) {
        debug!("add_live_point({:?}, {:?})", v, point);
        let definition = &mut self.definitions[v];
        if definition.value.add_point(point) {
            if definition.capped {
                self.errors.push(InferenceError {
                    constraint_point: point,
                    name: definition.name,
                });
            }
        }
    }

    pub fn add_outlives(&mut self, sup: RegionIndex, sub: RegionIndex, point: Location) {
        debug!("add_outlives({:?}: {:?} @ {:?}", sup, sub, point);
        self.constraints.push(Constraint { sup, sub, point });
    }

    pub fn region(&self, v: RegionIndex) -> &Region {
        &self.definitions[v].value
    }

    pub fn solve<'a, 'gcx, 'tcx>(
        &mut self,
        infcx: InferCtxt<'a, 'gcx, 'tcx>,
        mir: &'a Mir<'tcx>,
    ) -> IndexVec<InferenceErrorIndex, InferenceError>
    where
        'gcx: 'tcx + 'a,
        'tcx: 'a,
    {
        let mut changed = true;
        let mut dfs = Dfs::new(infcx, mir);
        while changed {
            changed = false;
            for constraint in &self.constraints {
                let sub = &self.definitions[constraint.sub].value.clone();
                let sup_def = &self.definitions[constraint.sup];
                debug!("constraint: {:?}", constraint);
                debug!("    sub (before): {:?}", sub);
                debug!("    sup (before): {:?}", sup_def.value);

                if dfs.copy(sub, &mut sup_def.value, constraint.point) {
                    changed = true;
                    if sup_def.capped {
                        // This is kind of a hack, but when we add a
                        // constraint, the "point" is always the point
                        // AFTER the action that induced the
                        // constraint. So report the error on the
                        // action BEFORE that.
                        assert!(constraint.point.statement_index > 0);
                        let p = Location {
                            block: constraint.point.block,
                            statement_index: constraint.point.statement_index - 1,
                        };

                        self.errors.push(InferenceError {
                            constraint_point: p,
                            name: sup_def.name,
                        });
                    }
                }

                debug!("    sup (after) : {:?}", sup_def.value);
                debug!("    changed     : {:?}", changed);
            }
            debug!("\n");
        }

        mem::replace(&mut self.errors, IndexVec::new())
    }
}

struct Dfs<'a, 'gcx: 'tcx + 'a, 'tcx: 'a> {
    infcx: InferCtxt<'a, 'gcx, 'tcx>,
    mir: &'a Mir<'tcx>,
}

impl<'a, 'gcx: 'tcx, 'tcx: 'a> Dfs<'a, 'gcx, 'tcx> {
    fn new(infcx: InferCtxt<'a, 'gcx, 'tcx>, mir: &'a Mir<'tcx>) -> Self {
        Self { infcx, mir }
    }

    fn copy(
        &mut self,
        from_region: &Region,
        to_region: &mut Region,
        start_point: Location,
    ) -> bool {
        let mut changed = false;

        let mut stack = vec![];
        let mut visited = FxHashSet();

        stack.push(start_point);
        while let Some(p) = stack.pop() {
            debug!("        dfs: p={:?}", p);

            if !from_region.may_contain(p) {
                debug!("            not in from-region");
                continue;
            }

            if !visited.insert(p) {
                debug!("            already visited");
                continue;
            }

            changed |= to_region.add_point(p);

            let block_data = self.mir[p.block];
            let successor_points = if p.statement_index < block_data.statements.len() {
                vec![Location {
                    statement_index: p.statement_index + 1,
                    ..p
                }]
            } else {
                block_data.terminator()
                    .successors()
                    .iter()
                    .map(|&basic_block| Location {
                        statement_index: 0,
                        block: basic_block,
                    })
                    .collect::<Vec<_>>()
            };

            if successor_points.is_empty() {
                // If we reach the END point in the graph, then copy
                // over any skolemized end points in the `from_region`
                // and make sure they are included in the `to_region`.
                for region_decl in self.infcx.tcx.tables.borrow().free_region_map() {
                    // TODO(nashenas88) figure out skolemized_end points
                    let block = self.env.graph.skolemized_end(region_decl.name);
                    let skolemized_end_point = Location {
                        block,
                        statement_index: 0,
                    };
                    changed |= to_region.add_point(skolemized_end_point);
                }
            } else {
                stack.extend(successor_points);
            }
        }

        changed
    }
}
