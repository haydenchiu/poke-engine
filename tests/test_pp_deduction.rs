#![cfg(not(any(feature = "gen1", feature = "gen2", feature = "gen3")))]
//! PP spent by one use of a move, against Showdown (gen9championsbssregmc) measured by
//! pokemon_rl scripts/sim_engine_fix/showdown_pp_rules.js -> experiments/084_engine_fixes_e1_e4/
//! showdown_pp_rules.json. Showdown: `runMove` -> `deductPP(move)` takes 1 PP; `useMove` then takes 1
//! more per opposing Pressure target; `deductPP` never takes PP below 0.
//!
//! Tests that start at PP < 10 hold with and without the `exact_pp` feature (the engine only tracks PP
//! below 10 by default); tests at PP >= 10 are gated on `exact_pp`.

use poke_engine::choices::Choices;
use poke_engine::engine::abilities::Abilities;
use poke_engine::engine::generate_instructions::generate_instructions_from_move_pair;
use poke_engine::engine::state::{MoveChoice, PokemonVolatileStatus};
use poke_engine::instruction::{Instruction, StateInstructions};
use poke_engine::state::{PokemonMoveIndex, PokemonStatus, PokemonType, State};

struct Turn {
    branches: Vec<StateInstructions>,
    /// side one's M0 PP after applying each branch
    pp_after: Vec<i8>,
}

/// Side one (faster) uses `mv` from slot M0 starting at `pp`; side two uses Splash. `setup` adjusts the
/// state before the turn is generated.
fn use_move(mv: Choices, pp: i8, setup: impl FnOnce(&mut State)) -> Turn {
    let mut state = State::default();
    let u = state.side_one.get_active();
    u.replace_move(PokemonMoveIndex::M0, mv);
    u.moves.m0.pp = pp;
    u.speed = 200;
    let t = state.side_two.get_active();
    t.replace_move(PokemonMoveIndex::M0, Choices::SPLASH);
    t.speed = 100;
    t.hp = 500;
    t.maxhp = 500;
    setup(&mut state);
    let before = format!("{:?}", state);
    let branches = generate_instructions_from_move_pair(
        &mut state,
        &MoveChoice::Move(PokemonMoveIndex::M0),
        &MoveChoice::Move(PokemonMoveIndex::M0),
        false,
    );
    assert_eq!(
        format!("{:?}", state),
        before,
        "generation must leave the state unchanged"
    );
    let mut pp_after = vec![];
    for b in &branches {
        state.apply_instructions(&b.instruction_list);
        pp_after.push(state.side_one.get_active_immutable().moves.m0.pp);
        state.reverse_instructions(&b.instruction_list);
    }
    Turn { branches, pp_after }
}

fn pressure(state: &mut State) {
    state.side_two.get_active().ability = Abilities::PRESSURE;
}

/// Every branch ends at the same PP; return it.
fn spent(turn: &Turn, start: i8) -> i8 {
    let first = turn.pp_after[0];
    assert!(
        turn.pp_after.iter().all(|p| *p == first),
        "branches disagree: {:?}",
        turn.pp_after
    );
    start - first
}

fn decrement_pp_count(turn: &Turn) -> usize {
    turn.branches
        .iter()
        .flat_map(|b| b.instruction_list.iter())
        .filter(|i| matches!(i, Instruction::DecrementPP(_)))
        .count()
}

// ---------------------------------------------------------------- E2: Pressure and the floor at 0 PP

/// The T3 reproducer (sim/tests/engine_discrepancies.rs e2): a 1-PP move into Pressure ends at 0
/// (Showdown pressure_tbolt_pp1: 1 -> 0). The pin took it to -1.
#[test]
fn test_pressure_on_last_pp_stops_at_zero() {
    let t = use_move(Choices::STONEEDGE, 1, pressure);
    assert_eq!(spent(&t, 1), 1);
    assert!(t.pp_after.iter().all(|p| *p == 0));
}

/// Showdown pressure_tbolt_pp2: 2 -> 0 (the full 2).
#[test]
fn test_pressure_on_two_pp_spends_both() {
    let t = use_move(Choices::THUNDERBOLT, 2, pressure);
    assert_eq!(spent(&t, 2), 2);
}

#[test]
fn test_pressure_costs_two() {
    let t = use_move(Choices::THUNDERBOLT, 5, pressure);
    assert_eq!(spent(&t, 5), 2);
}

/// Ordinary deduction: 1 PP, including the last one (Showdown base_tbolt_pp9 / pp1).
#[test]
fn test_ordinary_use_costs_one() {
    for start in [9i8, 5, 1] {
        let t = use_move(Choices::THUNDERBOLT, start, |_| {});
        assert_eq!(spent(&t, start), 1, "start {}", start);
    }
}

/// A move at 0 PP that is used anyway (the engine does not offer it, but generation accepts it) spends
/// nothing and emits no DecrementPP: Showdown's deductPP returns 0 at 0 PP.
#[test]
fn test_zero_pp_emits_no_decrement() {
    for setup in [(|_: &mut State| {}) as fn(&mut State), pressure] {
        let t = use_move(Choices::THUNDERBOLT, 0, setup);
        assert_eq!(decrement_pp_count(&t), 0);
        assert!(t.pp_after.iter().all(|p| *p == 0));
    }
}

/// A miss still costs the move's PP, Pressure included (Showdown miss: 8 -> 7).
#[test]
fn test_miss_branch_spends_pp() {
    let t = use_move(Choices::STONEEDGE, 8, pressure);
    assert!(t.branches.len() >= 2, "expected hit and miss branches");
    assert_eq!(spent(&t, 8), 2);
}

/// Immune target: the move is used, so PP and Pressure are spent (Showdown immune: Earthquake into
/// Corviknight 12 -> 10).
#[test]
fn test_immune_target_still_costs_pressure() {
    let t = use_move(Choices::EARTHQUAKE, 8, |s| {
        pressure(s);
        s.side_two.get_active().types = (PokemonType::FLYING, PokemonType::STEEL);
    });
    assert_eq!(spent(&t, 8), 2);
}

/// Pressure by move target, against Showdown's pressure_target_* cases.
#[test]
fn test_pressure_applicability_by_target() {
    for (mv, expected) in [
        (Choices::SWORDSDANCE, 1), // self
        (Choices::REFLECT, 1),     // allySide
        (Choices::RAINDANCE, 2),   // all
        (Choices::TRICKROOM, 2),   // all
        (Choices::HAZE, 2),        // all
        (Choices::SPIKES, 2),      // foeSide + mustpressure
        (Choices::STEALTHROCK, 2), // foeSide + mustpressure
        (Choices::STICKYWEB, 1),   // foeSide, no mustpressure (pin: 2)
        (Choices::IMPRISON, 2),    // self + mustpressure (pin: 1)
        (Choices::TOXIC, 2),       // normal
    ] {
        let t = use_move(mv, 8, pressure);
        assert_eq!(spent(&t, 8), expected, "{:?}", mv);
    }
}

/// A fainted foe is not a target, so its Pressure costs nothing (Showdown getMoveTargets returns no
/// pressureTargets for a fainted target). Status moves still run against a fainted foe in the engine.
#[test]
fn test_fainted_pressure_foe_adds_nothing() {
    let t = use_move(Choices::TOXIC, 8, |s| {
        pressure(s);
        s.side_two.get_active().hp = 0;
    });
    assert_eq!(spent(&t, 8), 1);
}

/// Pressure on the user's own side does nothing.
#[test]
fn test_own_pressure_adds_nothing() {
    let t = use_move(Choices::THUNDERBOLT, 5, |s| {
        s.side_one.get_active().ability = Abilities::PRESSURE;
    });
    assert_eq!(spent(&t, 5), 1);
}

// ------------------------------------------- E5: PP is spent only when the move is actually used
//
// Showdown runs BeforeMove (full paralysis, sleep, freeze, confusion, flinch, Taunt) before deductPP, so
// a Pokemon that cannot move spends nothing (showdown_pp_rules.json full_paralysis, asleep, frozen,
// confusion_self_hit, flinched, taunted_status_move: 0 PP). The pin pushed the DecrementPP before the
// status branches, so every branch - including "fully paralyzed" - spent it.

struct Branch {
    percentage: f32,
    /// side one's PP in slots M0 and M1 after the branch
    pp: (i8, i8),
    /// side one's move reached side two (side two lost HP)
    hit: bool,
}

fn branches(mv: Choices, pp: i8, setup: impl FnOnce(&mut State)) -> Vec<Branch> {
    let mut state = State::default();
    let u = state.side_one.get_active();
    u.replace_move(PokemonMoveIndex::M0, mv);
    u.moves.m0.pp = pp;
    u.speed = 200;
    let t = state.side_two.get_active();
    t.replace_move(PokemonMoveIndex::M0, Choices::SPLASH);
    t.speed = 100;
    t.hp = 500;
    t.maxhp = 500;
    setup(&mut state);
    let before = format!("{:?}", state);
    let generated = generate_instructions_from_move_pair(
        &mut state,
        &MoveChoice::Move(PokemonMoveIndex::M0),
        &MoveChoice::Move(PokemonMoveIndex::M0),
        false,
    );
    assert_eq!(
        format!("{:?}", state),
        before,
        "generation must leave the state unchanged"
    );
    let mut out = vec![];
    for b in &generated {
        state.apply_instructions(&b.instruction_list);
        let a = state.side_one.get_active_immutable();
        out.push(Branch {
            percentage: b.percentage,
            pp: (a.moves.m0.pp, a.moves.m1.pp),
            hit: state.side_two.get_active_immutable().hp < 500,
        });
        state.reverse_instructions(&b.instruction_list);
    }
    let total: f32 = out.iter().map(|b| b.percentage).sum();
    assert!((total - 100.0).abs() < 1e-2, "branch total {}", total);
    out
}

/// Asserts that branches where the move hit spent `used` PP from M0 and the others spent none, and that
/// both kinds occurred.
fn assert_used_spend(bs: &[Branch], start: i8, used: i8) {
    assert!(
        bs.iter().any(|b| b.hit) && bs.iter().any(|b| !b.hit),
        "expected both outcomes"
    );
    for b in bs {
        let expected = if b.hit { start - used } else { start };
        assert_eq!(b.pp.0, expected, "hit={} p={}", b.hit, b.percentage);
    }
}

#[test]
fn test_full_paralysis_spends_no_pp() {
    let bs = branches(Choices::TACKLE, 8, |s| {
        s.side_one.get_active().status = PokemonStatus::PARALYZE;
    });
    assert_used_spend(&bs, 8, 1);
}

#[test]
fn test_full_paralysis_into_pressure_spends_no_pp() {
    let bs = branches(Choices::TACKLE, 8, |s| {
        s.side_one.get_active().status = PokemonStatus::PARALYZE;
        pressure(s);
    });
    assert_used_spend(&bs, 8, 2);
}

#[test]
fn test_still_frozen_spends_no_pp() {
    let bs = branches(Choices::TACKLE, 8, |s| {
        s.side_one.get_active().status = PokemonStatus::FREEZE;
    });
    assert_used_spend(&bs, 8, 1);
}

/// sleep_turns 0: certain to stay asleep this turn - the only branch spends nothing.
#[test]
fn test_asleep_spends_no_pp() {
    let bs = branches(Choices::TACKLE, 8, |s| {
        s.side_one.get_active().status = PokemonStatus::SLEEP;
        s.side_one.get_active().sleep_turns = 0;
    });
    assert!(bs.iter().all(|b| !b.hit && b.pp.0 == 8));
}

/// sleep_turns 1: may wake. Woke -> the move is used (1 PP); stayed asleep -> nothing.
#[test]
fn test_may_wake_spends_pp_only_when_awake() {
    let bs = branches(Choices::TACKLE, 8, |s| {
        s.side_one.get_active().status = PokemonStatus::SLEEP;
        s.side_one.get_active().sleep_turns = 1;
    });
    assert_used_spend(&bs, 8, 1);
}

/// Rest sleep: still asleep (rest_turns 3) spends nothing; waking this turn (rest_turns 1) uses the move.
#[test]
fn test_rest_sleep() {
    let bs = branches(Choices::TACKLE, 8, |s| {
        s.side_one.get_active().status = PokemonStatus::SLEEP;
        s.side_one.get_active().rest_turns = 3;
    });
    assert!(bs.iter().all(|b| !b.hit && b.pp.0 == 8));
    let bs = branches(Choices::TACKLE, 8, |s| {
        s.side_one.get_active().status = PokemonStatus::SLEEP;
        s.side_one.get_active().rest_turns = 1;
    });
    assert!(bs.iter().all(|b| b.hit && b.pp.0 == 7));
}

#[test]
fn test_confusion_self_hit_spends_no_pp() {
    let bs = branches(Choices::TACKLE, 8, |s| {
        s.side_one
            .volatile_statuses
            .insert(PokemonVolatileStatus::CONFUSION);
    });
    assert_used_spend(&bs, 8, 1);
}

/// Paralysis and confusion together: only the branch that is neither fully paralyzed nor hits itself
/// spends PP.
#[test]
fn test_paralysis_and_confusion_together() {
    let bs = branches(Choices::TACKLE, 8, |s| {
        s.side_one.get_active().status = PokemonStatus::PARALYZE;
        s.side_one
            .volatile_statuses
            .insert(PokemonVolatileStatus::CONFUSION);
    });
    assert_used_spend(&bs, 8, 1);
    assert!(bs.iter().filter(|b| !b.hit).count() >= 2);
}

/// Unchanged: flinching was already checked before PP.
#[test]
fn test_flinch_spends_no_pp() {
    let bs = branches(Choices::TACKLE, 8, |s| {
        s.side_one
            .volatile_statuses
            .insert(PokemonVolatileStatus::FLINCH);
    });
    assert!(bs.iter().all(|b| !b.hit && b.pp.0 == 8));
}

/// A consecutive Protect that fails was still used: PP is spent in both the success and fail branches
/// (Showdown protect_consecutive_fail: 8 -> 7).
#[test]
fn test_failed_consecutive_protect_spends_pp() {
    let bs = branches(Choices::PROTECT, 8, |s| {
        s.side_one.side_conditions.protect = 1;
    });
    assert!(bs.len() >= 2);
    assert!(bs.iter().all(|b| b.pp.0 == 7));
}

/// Sleep Talk (M0) calling Tackle (M1): Sleep Talk pays 1 PP and the called move nothing (Showdown
/// sleeptalk: 12 -> 11, Thunderbolt untouched). The pin charged the called move to the wrong slot.
#[test]
fn test_sleep_talk_charges_only_sleep_talk() {
    let bs = branches(Choices::SLEEPTALK, 8, |s| {
        let u = s.side_one.get_active();
        u.replace_move(PokemonMoveIndex::M1, Choices::TACKLE);
        u.moves.m1.pp = 5;
        for m in [PokemonMoveIndex::M2, PokemonMoveIndex::M3] {
            u.replace_move(m, Choices::NONE);
        }
        u.status = PokemonStatus::SLEEP;
        u.sleep_turns = 0;
    });
    assert!(bs.iter().any(|b| b.hit));
    assert!(
        bs.iter().all(|b| b.pp == (7, 5)),
        "{:?}",
        bs.iter().map(|b| b.pp).collect::<Vec<_>>()
    );
}

/// Sleep Talk into Pressure: the called move targets the Pressure foe, so Sleep Talk pays 2 (Showdown
/// sleeptalk_pressure: 12 -> 10, Thunderbolt untouched).
#[test]
fn test_sleep_talk_pays_called_moves_pressure() {
    let bs = branches(Choices::SLEEPTALK, 8, |s| {
        let u = s.side_one.get_active();
        u.replace_move(PokemonMoveIndex::M1, Choices::TACKLE);
        u.moves.m1.pp = 5;
        for m in [PokemonMoveIndex::M2, PokemonMoveIndex::M3] {
            u.replace_move(m, Choices::NONE);
        }
        u.status = PokemonStatus::SLEEP;
        u.sleep_turns = 0;
        pressure(s);
    });
    assert!(
        bs.iter().all(|b| b.pp == (6, 5)),
        "{:?}",
        bs.iter().map(|b| b.pp).collect::<Vec<_>>()
    );
}

/// Charge moves: the first (charging) turn costs PP, the second (attacking) turn nothing, Pressure
/// included (Showdown multi_solarbeam [1, 0], multi_solarbeam_pressure [2, 0]).
#[test]
fn test_charge_move_costs_pp_on_first_turn_only() {
    for (with_pressure, first) in [(false, 1i8), (true, 2)] {
        let turn_one = branches(Choices::SOLARBEAM, 8, |s| {
            if with_pressure {
                pressure(s);
            }
        });
        assert!(
            turn_one.iter().all(|b| b.pp.0 == 8 - first),
            "turn 1, pressure={}",
            with_pressure
        );
        let turn_two = branches(Choices::SOLARBEAM, 8, |s| {
            if with_pressure {
                pressure(s);
            }
            s.side_one
                .volatile_statuses
                .insert(PokemonVolatileStatus::SOLARBEAM);
        });
        assert!(turn_two.iter().any(|b| b.hit));
        assert!(
            turn_two.iter().all(|b| b.pp.0 == 8),
            "turn 2, pressure={}",
            with_pressure
        );
    }
}

/// Outrage: the first turn costs PP, the locked turns nothing, Pressure included (Showdown
/// multi_outrage [1, 0], multi_outrage_pressure [2, 0]).
#[test]
fn test_locked_move_costs_pp_on_first_turn_only() {
    for (with_pressure, first) in [(false, 1i8), (true, 2)] {
        let turn_one = branches(Choices::OUTRAGE, 8, |s| {
            if with_pressure {
                pressure(s);
            }
        });
        assert!(turn_one.iter().all(|b| b.pp.0 == 8 - first));
        let locked = branches(Choices::OUTRAGE, 8, |s| {
            if with_pressure {
                pressure(s);
            }
            s.side_one
                .volatile_statuses
                .insert(PokemonVolatileStatus::LOCKEDMOVE);
        });
        assert!(locked.iter().any(|b| b.hit));
        assert!(
            locked.iter().all(|b| b.pp.0 == 8),
            "locked, pressure={}",
            with_pressure
        );
    }
}
