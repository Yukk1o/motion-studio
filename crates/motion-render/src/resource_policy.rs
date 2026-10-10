//! Application memory policy, not an adapter memory-capacity query.
pub const MAX_SCRATCH_BUDGET: u64 = 384 * 1024 * 1024;
pub fn scratch_budget(total_mem: u64, guarded: bool) -> u64 {
    const GIB: u64 = 1024 * 1024 * 1024;
    if guarded || total_mem == 0 {
        return motion_effects::SCRATCH_BUDGET_FLOOR;
    }
    if total_mem <= 3 * GIB {
        96 * 1024 * 1024
    } else if total_mem <= 6 * GIB {
        192 * 1024 * 1024
    } else {
        MAX_SCRATCH_BUDGET
    }
}
