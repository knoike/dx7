//! DX7 algorithm definitions and topological sort.
//!
//! The DX7 has 32 algorithm topologies defining how 6 operators interconnect.
//! Operators are numbered 1-6 in DX7 documentation but 0-5 in code.
//! Each algorithm specifies:
//! - Which operators are carriers (output to audio)
//! - Which operator pairs form modulator→carrier connections
//! - Which operator has self-feedback

/// Definition of a single DX7 algorithm topology.
pub struct AlgorithmDef {
    /// Operator indices that contribute to audio output (carriers).
    pub carriers: &'static [usize],
    /// Modulation connections: (source_op, dest_op) — source modulates dest.
    pub modulations: &'static [(usize, usize)],
    /// Which operator has self-feedback (0-5).
    pub feedback_op: usize,
}

/// All 32 DX7 algorithms.
/// Operator numbering: 0=OP1, 1=OP2, 2=OP3, 3=OP4, 4=OP5, 5=OP6
/// In DX7 docs these are OP1-OP6 (1-indexed).
pub static ALGORITHMS: [AlgorithmDef; 32] = [
    // Algorithm 1: 6→5→4→3→2→1  (fb on 6)
    AlgorithmDef {
        carriers: &[0],
        modulations: &[(1, 0), (2, 1), (3, 2), (4, 3), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 2: 6→5→4→3→2→1  (fb on 2)
    AlgorithmDef {
        carriers: &[0],
        modulations: &[(1, 0), (2, 1), (3, 2), (4, 3), (5, 4)],
        feedback_op: 1,
    },
    // Algorithm 3: 6→5→4→{3→2→1}  (fb on 6) — op3 also modulates op1
    AlgorithmDef {
        carriers: &[0],
        modulations: &[(1, 0), (2, 0), (3, 2), (4, 3), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 4: 6→5→4→3→2→1  (fb on 6, op6 detune)
    AlgorithmDef {
        carriers: &[0],
        modulations: &[(1, 0), (2, 1), (3, 2), (4, 3), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 5: 6→5, 4→3, 2→1  — three 2-op pairs (fb on 6)
    AlgorithmDef {
        carriers: &[0, 2, 4],
        modulations: &[(1, 0), (3, 2), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 6: 6→5, 4→3, 2→1  (fb on 6, variable feedback)
    AlgorithmDef {
        carriers: &[0, 2, 4],
        modulations: &[(1, 0), (3, 2), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 7: 6→5, {4,3}→2→1  (fb on 6) — op3 and op4 both mod op2
    AlgorithmDef {
        carriers: &[0],
        modulations: &[(1, 0), (2, 1), (3, 1), (4, 3), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 8: 4→3→2→1, 6→5  (fb on 4)
    AlgorithmDef {
        carriers: &[0, 4],
        modulations: &[(1, 0), (2, 1), (3, 2), (5, 4)],
        feedback_op: 3,
    },
    // Algorithm 9: 4→3→2→1, 6→5  (fb on 2)
    AlgorithmDef {
        carriers: &[0, 4],
        modulations: &[(1, 0), (2, 1), (3, 2), (5, 4)],
        feedback_op: 1,
    },
    // Algorithm 10: 3→2→1, 6→{5,4}  (fb on 3)
    AlgorithmDef {
        carriers: &[0, 3, 4],
        modulations: &[(1, 0), (2, 1), (5, 3), (5, 4)],
        feedback_op: 2,
    },
    // Algorithm 11: 6→5, 3→2→1, 4 carrier  (fb on 6)
    AlgorithmDef {
        carriers: &[0, 3, 4],
        modulations: &[(1, 0), (2, 1), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 12: 2→1, 4→3, 6→5  (fb on 2)
    AlgorithmDef {
        carriers: &[0, 2, 4],
        modulations: &[(1, 0), (3, 2), (5, 4)],
        feedback_op: 1,
    },
    // Algorithm 13: 2→1, {6,5,4}→3  (fb on 2)
    AlgorithmDef {
        carriers: &[0, 2],
        modulations: &[(1, 0), (3, 2), (4, 2), (5, 2)],
        feedback_op: 1,
    },
    // Algorithm 14: 2→1, 6→5→{4,3}  (fb on 6)
    AlgorithmDef {
        carriers: &[0, 2],
        modulations: &[(1, 0), (4, 2), (5, 3), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 15: 2→1, 6→5→4→3  (fb on 2)
    AlgorithmDef {
        carriers: &[0, 2],
        modulations: &[(1, 0), (3, 2), (4, 3), (5, 4)],
        feedback_op: 1,
    },
    // Algorithm 16: 6→5→4→{3,2,1}  (fb on 6) — op4 mods all of 1,2,3
    AlgorithmDef {
        carriers: &[0],
        modulations: &[(1, 0), (2, 0), (3, 0), (4, 1), (4, 2), (4, 3), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 17: {6→5→4, 3}→2→1  (fb on 6) — both op3 and op4 mod op2
    AlgorithmDef {
        carriers: &[0],
        modulations: &[(1, 0), (2, 0), (3, 1), (4, 3), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 18: 6→5→{4→3, 2}→1  (fb on 6)
    AlgorithmDef {
        carriers: &[0],
        modulations: &[(1, 0), (2, 0), (3, 2), (4, 1), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 19: 6→{5,4,3}, 2→1  (fb on 6) — 3 carriers from one mod chain
    AlgorithmDef {
        carriers: &[0, 2, 3, 4],
        modulations: &[(1, 0), (5, 2), (5, 3), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 20: {3→2, 6→5}→1, 4 carrier  (fb on 3)
    AlgorithmDef {
        carriers: &[0, 3, 4],
        modulations: &[(1, 0), (2, 0), (4, 0), (5, 4)],
        feedback_op: 2,
    },
    // Algorithm 21: {3→2, 6→5, 4}→1  (fb on 3)
    AlgorithmDef {
        carriers: &[0, 3, 4],
        modulations: &[(1, 0), (2, 0), (4, 0), (5, 3), (5, 4)],
        feedback_op: 2,
    },
    // Algorithm 22: 6→{5,4,3}, 2→1  (fb on 6) — 4 carriers
    AlgorithmDef {
        carriers: &[0, 2, 3, 4],
        modulations: &[(1, 0), (5, 2), (5, 3), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 23: {6→5, 4}→3, 2→1  (fb on 6)
    AlgorithmDef {
        carriers: &[0, 2, 3],
        modulations: &[(1, 0), (3, 2), (4, 2), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 24: 6→{5,4,3,2,1}  (fb on 6) — all carriers except 6
    AlgorithmDef {
        carriers: &[0, 1, 2, 3, 4],
        modulations: &[(5, 0), (5, 1), (5, 2), (5, 3), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 25: 6→{5,4}, 3→2, 1 carrier  (fb on 6)
    AlgorithmDef {
        carriers: &[0, 1, 2, 3, 4],
        modulations: &[(5, 3), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 26: 6→5→4, 3→2, 1 carrier  (fb on 6)
    AlgorithmDef {
        carriers: &[0, 1, 3],
        modulations: &[(2, 1), (4, 3), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 27: 3→2, 6→5, 1, 4 carriers  (fb on 3)
    AlgorithmDef {
        carriers: &[0, 1, 3, 4],
        modulations: &[(2, 1), (5, 4)],
        feedback_op: 2,
    },
    // Algorithm 28: 6→5→4, 3, 2, 1  (fb on 6)  — 5→3 with 2,1 carriers
    AlgorithmDef {
        carriers: &[0, 1, 2, 4],
        modulations: &[(2, 1), (5, 4), (4, 3)],
        feedback_op: 5,
    },
    // Algorithm 29: 6→5, 4→3, 2, 1  (fb on 6)
    AlgorithmDef {
        carriers: &[0, 1, 2, 4],
        modulations: &[(3, 2), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 30: 6→5→4, 3, 2, 1  (fb on 6)
    AlgorithmDef {
        carriers: &[0, 1, 2, 3],
        modulations: &[(4, 3), (5, 4)],
        feedback_op: 5,
    },
    // Algorithm 31: 6→5, 4, 3, 2, 1  (fb on 6) — 5 carriers
    AlgorithmDef {
        carriers: &[0, 1, 2, 3, 4],
        modulations: &[(5, 4)],
        feedback_op: 5,
    },
    // Algorithm 32: all 6 carriers, pure additive  (fb on 6)
    AlgorithmDef {
        carriers: &[0, 1, 2, 3, 4, 5],
        modulations: &[],
        feedback_op: 5,
    },
];

/// Compute topological processing order for an algorithm.
/// Returns operator indices in the order they should be computed
/// (modulators before their targets).
pub fn compute_processing_order(alg: &AlgorithmDef) -> [usize; 6] {
    let mut order = [0usize; 6];
    let mut visited = [false; 6];
    let mut pos = 0;

    // Build adjacency: who modulates whom
    // modulations are (src, dst), src must be computed before dst
    let mut deps: [u8; 6] = [0; 6]; // bitmask of dependencies for each op

    for &(src, dst) in alg.modulations {
        deps[dst] |= 1 << src;
    }

    // Kahn's algorithm: repeatedly pick ops with no unresolved deps
    for _ in 0..6 {
        // Find an unvisited op with all deps satisfied
        for op in (0..6).rev() {
            // Process higher-numbered ops first (matches DX7 convention)
            if visited[op] {
                continue;
            }
            // Check if all dependencies are visited
            let mut deps_met = true;
            for dep in 0..6 {
                if deps[op] & (1 << dep) != 0 && !visited[dep] {
                    deps_met = false;
                    break;
                }
            }
            if deps_met {
                order[pos] = op;
                pos += 1;
                visited[op] = true;
                break;
            }
        }
    }

    order
}

/// Check if an operator is a carrier in the given algorithm.
pub fn is_carrier(alg: &AlgorithmDef, op: usize) -> bool {
    alg.carriers.contains(&op)
}

/// Get all operators that modulate a given target operator.
pub fn get_modulators(alg: &AlgorithmDef, target: usize) -> [bool; 6] {
    let mut result = [false; 6];
    for &(src, dst) in alg.modulations {
        if dst == target {
            result[src] = true;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_algorithm_1_processing_order() {
        let order = compute_processing_order(&ALGORITHMS[0]);
        // Alg 1: 6→5→4→3→2→1, so order should be 5,4,3,2,1,0
        // (higher numbered ops processed first as they are modulators)
        assert_eq!(order[0], 5); // OP6 first (no deps)
        assert_eq!(order[5], 0); // OP1 last (carrier)
    }

    #[test]
    fn test_algorithm_32_all_carriers() {
        let alg = &ALGORITHMS[31];
        assert_eq!(alg.carriers.len(), 6);
        assert!(alg.modulations.is_empty());
    }

    #[test]
    fn test_algorithm_5_three_pairs() {
        let alg = &ALGORITHMS[4];
        assert_eq!(alg.carriers.len(), 3);
        assert_eq!(alg.modulations.len(), 3);
    }

    #[test]
    fn test_processing_order_valid() {
        // Verify that for every algorithm, modulators come before their targets
        for (i, alg) in ALGORITHMS.iter().enumerate() {
            let order = compute_processing_order(alg);
            let mut position = [0usize; 6];
            for (pos, &op) in order.iter().enumerate() {
                position[op] = pos;
            }
            for &(src, dst) in alg.modulations {
                assert!(
                    position[src] < position[dst],
                    "Algorithm {}: op{} should be before op{} (positions {} vs {})",
                    i + 1,
                    src + 1,
                    dst + 1,
                    position[src],
                    position[dst]
                );
            }
        }
    }
}
