#![cfg(not(any(feature = "gen1", feature = "gen2", feature = "gen3")))]
//! Struggle, against Showdown (gen9championsbssregmc) measured by pokemon_rl
//! scripts/sim_engine_fix/showdown_struggle_cases.js -> experiments/085_struggle/showdown_struggle_cases.json
//! (and exp084's showdown_struggle_rules.json). Stats below are Showdown's for each case's set (level 50).
//!
//! The tests reach Struggle only through the string API (`MoveChoice::from_string("struggle")`,
//! `MoveChoice::to_string`), so this file also compiles against an engine without Struggle and fails there
//! by assertion. Option-generation tests need the `struggle` feature; resolution tests run in every build.

use poke_engine::choices::Choices;
use poke_engine::engine::abilities::Abilities;
use poke_engine::engine::generate_instructions::generate_instructions_from_move_pair;
use poke_engine::engine::items::Items;
use poke_engine::engine::state::{MoveChoice, PokemonVolatileStatus};
use poke_engine::instruction::{Instruction, StateInstructions};
use poke_engine::state::{LastUsedMove, PokemonMoveIndex, PokemonType, SideReference, State};

const SLOTS: [PokemonMoveIndex; 4] = [
    PokemonMoveIndex::M0,
    PokemonMoveIndex::M1,
    PokemonMoveIndex::M2,
    PokemonMoveIndex::M3,
];

struct Mon {
    types: (PokemonType, PokemonType),
    attack: i16,
    defense: i16,
    maxhp: i16,
}

/// Side one (faster) has four moves at 0 PP and the given stats; side two uses `foe_move`.
fn state(user: Mon, foe: Mon, foe_move: Choices) -> State {
    let mut s = State::default();
    let u = s.side_one.get_active();
    for (slot, mv) in SLOTS.iter().zip([
        Choices::FLASHCANNON,
        Choices::DRACOMETEOR,
        Choices::DRAGONTAIL,
        Choices::STEALTHROCK,
    ]) {
        u.replace_move(*slot, mv);
        u.moves[slot].pp = 0;
    }
    u.level = 50;
    u.types = user.types;
    u.attack = user.attack;
    u.defense = user.defense;
    u.maxhp = user.maxhp;
    u.hp = user.maxhp;
    u.speed = 200;
    let t = s.side_two.get_active();
    t.replace_move(PokemonMoveIndex::M0, foe_move);
    t.level = 50;
    t.types = foe.types;
    t.attack = foe.attack;
    t.defense = foe.defense;
    t.maxhp = foe.maxhp;
    t.hp = foe.maxhp;
    t.speed = 100;
    s
}

fn archaludon() -> Mon {
    Mon {
        types: (PokemonType::STEEL, PokemonType::DRAGON),
        attack: 125,
        defense: 100,
        maxhp: 165,
    }
}
fn umbreon() -> Mon {
    // 252 HP / 252 Def, as in the oracle's "bulky" foe
    Mon {
        types: (PokemonType::DARK, PokemonType::TYPELESS),
        attack: 85,
        defense: 382,
        maxhp: 422,
    }
}

fn struggle(s: &State) -> MoveChoice {
    MoveChoice::from_string("struggle", &s.side_one)
        .expect("the engine has no Struggle choice (MoveChoice::from_string(\"struggle\") is None)")
}

fn splash(s: &State) -> MoveChoice {
    MoveChoice::from_string(
        &format!("{:?}", s.side_two.get_active_immutable().moves.m0.id),
        &s.side_two,
    )
    .unwrap()
}

/// Resolve one turn (no damage-roll branching: the engine's single representative roll).
fn turn(s: &mut State, s1: &MoveChoice, s2: &MoveChoice) -> Vec<StateInstructions> {
    let before = format!("{:?}", s);
    let b = generate_instructions_from_move_pair(s, s1, s2, false);
    assert_eq!(
        format!("{:?}", s),
        before,
        "generation must leave the state unchanged"
    );
    let total: f32 = b.iter().map(|x| x.percentage).sum();
    assert!((total - 100.0).abs() < 1e-2, "branch total {}", total);
    b
}

struct After {
    user_hp: i16,
    foe_hp: i16,
    foe_sub: i16,
    pp: [i8; 4],
    last_used: LastUsedMove,
    user_types: (PokemonType, PokemonType),
    disabled: [bool; 4],
}

fn apply(s: &mut State, b: &StateInstructions) -> After {
    s.apply_instructions(&b.instruction_list);
    let u = s.side_one.get_active_immutable();
    let a = After {
        user_hp: u.hp,
        foe_hp: s.side_two.get_active_immutable().hp,
        foe_sub: s.side_two.substitute_health,
        pp: [u.moves.m0.pp, u.moves.m1.pp, u.moves.m2.pp, u.moves.m3.pp],
        last_used: s.side_one.last_used_move,
        user_types: u.types,
        disabled: [
            u.moves.m0.disabled,
            u.moves.m1.disabled,
            u.moves.m2.disabled,
            u.moves.m3.disabled,
        ],
    };
    s.reverse_instructions(&b.instruction_list);
    a
}

/// One branch expected; returns (damage to side two, recoil to side one, after).
fn struggle_once(s: &mut State) -> (i16, i16, After) {
    let (s1, s2) = (struggle(s), splash(s));
    let b = turn(s, &s1, &s2);
    assert_eq!(b.len(), 1, "expected one branch");
    let (u0, f0) = (
        s.side_one.get_active_immutable().hp,
        s.side_two.get_active_immutable().hp,
    );
    let a = apply(s, &b[0]);
    (f0 - a.foe_hp, u0 - a.user_hp, a)
}

fn in_range(damage: i16, range: (i16, i16), case: &str) {
    assert!(
        damage >= range.0 && damage <= range.1,
        "{}: engine damage {} outside Showdown's non-crit range {:?}",
        case,
        damage,
        range
    );
}

fn option_strings(s: &State) -> Vec<String> {
    s.root_get_all_options()
        .0
        .iter()
        .map(|c| c.to_string(&s.side_one))
        .collect()
}

// ------------------------------------------------------------------------- availability (feature)

/// Every move at 0 PP: the only move option is Struggle (Showdown `request_moves == ['struggle']`), and
/// switching stays available (`struggle_can_switch`).
#[cfg(feature = "struggle")]
#[test]
fn test_all_moves_out_of_pp_offers_struggle_and_switches() {
    let s = state(archaludon(), umbreon(), Choices::SPLASH);
    let o = option_strings(&s);
    assert_eq!(o.iter().filter(|x| *x == "struggle").count(), 1, "{:?}", o);
    let moves: Vec<&String> = o
        .iter()
        .filter(|x| {
            ["flashcannon", "dracometeor", "dragontail", "stealthrock"]
                .iter()
                .any(|m| x.starts_with(m))
        })
        .collect();
    assert!(moves.is_empty(), "no 0-PP move may be offered: {:?}", o);
    assert!(o.len() > 1, "switches remain available: {:?}", o);
}

/// Trapped and out of PP: exactly Struggle.
#[cfg(feature = "struggle")]
#[test]
fn test_only_struggle_when_no_switch() {
    let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
    for i in 1..6 {
        s.side_one.pokemon.pkmn[i].hp = 0;
    }
    assert_eq!(option_strings(&s), vec!["struggle".to_string()]);
}

/// A move with PP left is offered and Struggle is not.
#[cfg(feature = "struggle")]
#[test]
fn test_move_with_pp_left_means_no_struggle() {
    for pp in [1i8, 16] {
        let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
        s.side_one.get_active().moves.m1.pp = pp;
        let o = option_strings(&s);
        assert!(
            o.contains(&"dracometeor".to_string()) && !o.contains(&"struggle".to_string()),
            "{:?}",
            o
        );
    }
}

/// Taunt with only status moves left (Showdown `taunted_only_status_moves`): Struggle.
#[cfg(feature = "struggle")]
#[test]
fn test_taunted_status_only_moveset_offers_struggle() {
    let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
    let u = s.side_one.get_active();
    u.replace_move(PokemonMoveIndex::M0, Choices::ROAR);
    u.replace_move(PokemonMoveIndex::M1, Choices::STEALTHROCK);
    u.moves.m0.pp = 16;
    u.moves.m1.pp = 16;
    s.side_one
        .volatile_statuses
        .insert(PokemonVolatileStatus::TAUNT);
    assert!(option_strings(&s).contains(&"struggle".to_string()));
}

/// No mega evolution with Struggle (Showdown `struggle_with_mega`: canMegaEvo false).
#[cfg(feature = "struggle")]
#[test]
fn test_no_mega_with_struggle() {
    let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
    s.side_one.get_active().id = poke_engine::pokemon::PokemonName::GENGAR;
    s.side_one.get_active().item = Items::GENGARITE;
    let o = option_strings(&s);
    assert!(o.contains(&"struggle".to_string()));
    assert!(!o.iter().any(|x| x.ends_with("-mega")), "{:?}", o);
    // control: the same Pokemon with a usable move is offered mega evolution, so the check above is live
    s.side_one.get_active().moves.m0.pp = 8;
    let o = option_strings(&s);
    assert!(o.iter().any(|x| x.ends_with("-mega")), "control: {:?}", o);
}

/// Not offered when the side is not choosing a move: recharging, or the opponent is replacing a
/// fainted Pokemon.
#[cfg(feature = "struggle")]
#[test]
fn test_no_struggle_when_not_choosing_a_move() {
    let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
    s.side_one
        .volatile_statuses
        .insert(PokemonVolatileStatus::MUSTRECHARGE);
    assert!(!option_strings(&s).contains(&"struggle".to_string()));
    let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
    s.side_two.get_active().hp = 0;
    assert_eq!(option_strings(&s), vec!["No Move".to_string()]);
}

/// Without the feature, option generation is unchanged: no Struggle is offered.
#[cfg(not(feature = "struggle"))]
#[test]
fn test_default_build_does_not_offer_struggle() {
    let s = state(archaludon(), umbreon(), Choices::SPLASH);
    assert!(!option_strings(&s).contains(&"struggle".to_string()));
}

// ------------------------------------------------------------------------- representation (every build)

#[test]
fn test_struggle_round_trips_as_a_string_and_is_not_a_slot() {
    let s = state(archaludon(), umbreon(), Choices::SPLASH);
    let c = struggle(&s);
    assert_eq!(c.to_string(&s.side_one), "struggle");
    for slot in SLOTS {
        for wrapped in [
            MoveChoice::Move(slot),
            MoveChoice::MoveMega(slot),
            MoveChoice::MoveTera(slot),
        ] {
            assert_ne!(c, wrapped);
        }
    }
    assert_ne!(c, MoveChoice::None);
}

// ------------------------------------------------------------------------- damage (every build)

/// Typeless: neutral into Ghost (a Normal move would do 0) - Showdown archaludon_into_gengar 8..10.
#[test]
fn test_struggle_hits_ghost_neutrally() {
    let gengar = Mon {
        types: (PokemonType::GHOST, PokemonType::POISON),
        attack: 85,
        defense: 332,
        maxhp: 387,
    };
    let mut s = state(archaludon(), gengar, Choices::SPLASH);
    let (dmg, _, _) = struggle_once(&mut s);
    in_range(dmg, (8, 10), "archaludon_into_gengar");
}

#[test]
fn test_struggle_damage_matches_showdown() {
    let (dmg, _, _) = struggle_once(&mut state(archaludon(), umbreon(), Choices::SPLASH));
    in_range(dmg, (7, 9), "archaludon_into_umbreon");
    let corviknight = Mon {
        types: (PokemonType::FLYING, PokemonType::STEEL),
        attack: 107,
        defense: 377,
        maxhp: 425,
    };
    let garchomp = Mon {
        types: (PokemonType::DRAGON, PokemonType::GROUND),
        attack: 150,
        defense: 115,
        maxhp: 183,
    };
    let (dmg, _, _) = struggle_once(&mut state(garchomp, corviknight, Choices::SPLASH));
    in_range(
        dmg,
        (8, 10),
        "garchomp_into_corviknight_steel (no Steel resistance)",
    );
}

/// No STAB for a single-typed Normal user (the engine stores its second type as TYPELESS): Showdown
/// snorlax_single_type_normal 7..9; with STAB it would be about 1.5x.
#[test]
fn test_struggle_has_no_stab_for_single_typed_user() {
    let snorlax = Mon {
        types: (PokemonType::NORMAL, PokemonType::TYPELESS),
        attack: 130,
        defense: 85,
        maxhp: 235,
    };
    let (dmg, _, _) = struggle_once(&mut state(snorlax, umbreon(), Choices::SPLASH));
    in_range(dmg, (7, 9), "snorlax_single_type_normal");
}

/// Technician applies (50 BP <= 60): Showdown 11..14 vs 8..10 without it.
#[test]
fn test_struggle_technician_applies() {
    let scizor = || Mon {
        types: (PokemonType::BUG, PokemonType::STEEL),
        attack: 150,
        defense: 120,
        maxhp: 145,
    };
    let mut s = state(scizor(), umbreon(), Choices::SPLASH);
    s.side_one.get_active().ability = Abilities::TECHNICIAN;
    in_range(struggle_once(&mut s).0, (11, 14), "scizor_technician");
    in_range(
        struggle_once(&mut state(scizor(), umbreon(), Choices::SPLASH)).0,
        (8, 10),
        "scizor_no_technician",
    );
}

/// Tough Claws applies (contact): Showdown 9..11.
#[test]
fn test_struggle_tough_claws_applies() {
    let barbaracle = Mon {
        types: (PokemonType::ROCK, PokemonType::WATER),
        attack: 125,
        defense: 135,
        maxhp: 147,
    };
    let mut s = state(barbaracle, umbreon(), Choices::SPLASH);
    s.side_one.get_active().ability = Abilities::TOUGHCLAWS;
    in_range(struggle_once(&mut s).0, (9, 11), "barbaracle_tough_claws");
}

/// Pixilate does not convert it (keys on Normal): Showdown 5..6.
#[test]
fn test_struggle_not_converted_by_pixilate() {
    let sylveon = Mon {
        types: (PokemonType::FAIRY, PokemonType::TYPELESS),
        attack: 85,
        defense: 85,
        maxhp: 170,
    };
    let mut s = state(sylveon, umbreon(), Choices::SPLASH);
    s.side_one.get_active().ability = Abilities::PIXILATE;
    in_range(struggle_once(&mut s).0, (5, 6), "sylveon_pixilate");
}

/// Protean does not change the user's type for a typeless move (Showdown: Greninja stays Water/Dark).
#[test]
fn test_struggle_does_not_trigger_protean() {
    let greninja = Mon {
        types: (PokemonType::WATER, PokemonType::DARK),
        attack: 115,
        defense: 87,
        maxhp: 147,
    };
    let mut s = state(greninja, umbreon(), Choices::SPLASH);
    s.side_one.get_active().ability = Abilities::PROTEAN;
    let (dmg, _, a) = struggle_once(&mut s);
    in_range(dmg, (6, 8), "greninja_protean");
    assert_eq!(a.user_types, (PokemonType::WATER, PokemonType::DARK));
}

/// Never misses: hits through +6 evasion (Showdown into_plus6_evasion).
#[test]
fn test_struggle_hits_through_evasion() {
    let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
    s.side_two.evasion_boost = 6;
    let (dmg, recoil, _) = struggle_once(&mut s);
    assert!(dmg > 0 && recoil == 41);
}

// ------------------------------------------------------------------------- recoil (every build)

/// round(maxhp / 4), halves rounding up: 165 -> 41, 235 -> 59, 183 -> 46, 422 -> 106 (105.5), 147 -> 37.
#[test]
fn test_struggle_recoil_is_a_quarter_of_max_hp_rounded() {
    for (maxhp, expected) in [
        (165i16, 41i16),
        (235, 59),
        (183, 46),
        (422, 106),
        (147, 37),
        (202, 51),
    ] {
        let user = Mon {
            maxhp,
            ..archaludon()
        };
        let (_, recoil, _) = struggle_once(&mut state(user, umbreon(), Choices::SPLASH));
        assert_eq!(recoil, expected, "maxhp {}", maxhp);
    }
}

/// At least 1, even for a tiny max HP.
#[test]
fn test_struggle_recoil_is_at_least_one() {
    for maxhp in [1i16, 2] {
        let user = Mon {
            maxhp,
            ..archaludon()
        };
        let (_, recoil, a) = struggle_once(&mut state(user, umbreon(), Choices::SPLASH));
        assert_eq!(recoil, 1, "maxhp {}", maxhp);
        assert!(a.user_hp >= 0);
    }
}

/// Recoil KO: the target is still damaged, the user's HP floors at 0 (Showdown recoil_ko).
#[test]
fn test_struggle_recoil_can_knock_out_the_user() {
    let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
    s.side_one.get_active().hp = 10;
    let (dmg, recoil, a) = struggle_once(&mut s);
    assert!(dmg > 0);
    assert_eq!((recoil, a.user_hp), (10, 0));
}

/// Magic Guard and Rock Head do not prevent it (Showdown 43, 39).
#[test]
fn test_struggle_recoil_ignores_magic_guard_and_rock_head() {
    for (ability, maxhp, expected) in [
        (Abilities::MAGICGUARD, 170i16, 43i16),
        (Abilities::ROCKHEAD, 155, 39),
    ] {
        let mut s = state(
            Mon {
                maxhp,
                ..archaludon()
            },
            umbreon(),
            Choices::SPLASH,
        );
        s.side_one.get_active().ability = ability;
        assert_eq!(struggle_once(&mut s).1, expected, "{:?}", ability);
    }
}

/// Reckless does not boost it (it has no `recoil` fraction): same damage as without.
#[test]
fn test_struggle_not_boosted_by_reckless() {
    let plain = struggle_once(&mut state(archaludon(), umbreon(), Choices::SPLASH)).0;
    let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
    s.side_one.get_active().ability = Abilities::RECKLESS;
    assert_eq!(struggle_once(&mut s).0, plain);
}

/// Blocked by Protect: no damage and no recoil (Showdown into_protect).
#[test]
fn test_struggle_into_protect_no_recoil() {
    let mut s = state(archaludon(), umbreon(), Choices::PROTECT);
    s.side_two.get_active().speed = 300; // Protect goes first
    let (s1, s2) = (struggle(&s), splash(&s));
    for b in turn(&mut s, &s1, &s2) {
        let a = apply(&mut s, &b);
        assert_eq!((a.user_hp, a.foe_hp), (165, 422));
    }
}

/// Into a Substitute: the Substitute takes the hit and the user still recoils (Showdown into_substitute).
#[test]
fn test_struggle_into_substitute_recoils() {
    let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
    s.side_two
        .volatile_statuses
        .insert(PokemonVolatileStatus::SUBSTITUTE);
    s.side_two.substitute_health = 200;
    let (_, recoil, a) = struggle_once(&mut s);
    assert!(a.foe_sub < 200 && a.foe_hp == 422);
    assert_eq!(recoil, 41);
}

/// Life Orb and Rocky Helmet add to the recoil (Showdown 41 + 16, 41 + 27).
#[test]
fn test_struggle_recoil_with_life_orb_and_rocky_helmet() {
    let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
    s.side_one.get_active().item = Items::LIFEORB;
    assert_eq!(struggle_once(&mut s).1, 41 + 16);
    let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
    s.side_two.get_active().item = Items::ROCKYHELMET;
    assert_eq!(struggle_once(&mut s).1, 41 + 27);
}

/// The foe fainted before Struggle ran: no target, no recoil (Showdown target_fainted_first).
#[test]
fn test_struggle_with_no_target_no_recoil() {
    let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
    s.side_two.get_active().hp = 0;
    let s1 = struggle(&s);
    for b in turn(&mut s, &s1, &MoveChoice::None) {
        assert_eq!(apply(&mut s, &b).user_hp, 165);
    }
}

// ------------------------------------------------------------------------- PP, last move, locks (every build)

/// No PP is spent, Pressure included (Showdown struggle_pressure): every slot stays at 0.
#[test]
fn test_struggle_spends_no_pp() {
    for pressure in [false, true] {
        let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
        if pressure {
            s.side_two.get_active().ability = Abilities::PRESSURE;
        }
        let (s1, s2) = (struggle(&s), splash(&s));
        for b in turn(&mut s, &s1, &s2) {
            assert!(!b.instruction_list.iter().any(
                |i| matches!(i, Instruction::DecrementPP(d) if d.side_ref == SideReference::SideOne)
            ));
            assert_eq!(apply(&mut s, &b).pp, [0, 0, 0, 0]);
        }
    }
}

/// Struggle chosen while slots still have PP (Taunt, status-only moveset): no slot loses PP. At 8 PP so the
/// check holds with and without `exact_pp`.
#[test]
fn test_struggle_under_taunt_spends_no_slot_pp() {
    let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
    let u = s.side_one.get_active();
    u.replace_move(PokemonMoveIndex::M0, Choices::ROAR);
    u.replace_move(PokemonMoveIndex::M1, Choices::STEALTHROCK);
    u.moves.m0.pp = 8;
    u.moves.m1.pp = 8;
    s.side_one
        .volatile_statuses
        .insert(PokemonVolatileStatus::TAUNT);
    let (dmg, recoil, a) = struggle_once(&mut s);
    assert!(dmg > 0 && recoil == 41);
    assert_eq!(a.pp, [8, 8, 0, 0]);
}

/// The last-used move is never a slot: recorded as None (Encore fails, no repeat lock, no Fake Out).
#[test]
fn test_struggle_last_used_move_is_not_a_slot() {
    let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
    s.use_last_used_move = true;
    s.side_one.last_used_move = LastUsedMove::Move(PokemonMoveIndex::M2);
    let (_, _, a) = struggle_once(&mut s);
    assert!(
        matches!(a.last_used, LastUsedMove::None),
        "{:?}",
        a.last_used
    );
}

/// Encore against a Pokemon whose last move was Struggle has no effect.
#[test]
fn test_encore_fails_after_struggle() {
    let mut s = state(archaludon(), umbreon(), Choices::ENCORE);
    s.use_last_used_move = true;
    s.side_one.last_used_move = LastUsedMove::None; // as recorded after Struggle
    s.side_two.get_active().speed = 300;
    let (s1, s2) = (struggle(&s), splash(&s));
    for b in turn(&mut s, &s1, &s2) {
        assert!(!b.instruction_list.iter().any(|i| matches!(i, Instruction::ApplyVolatileStatus(v) if v.volatile_status == PokemonVolatileStatus::ENCORE)));
    }
}

/// An encored Pokemon with no usable move Struggles; it is not turned into the 0-PP encored move.
#[test]
fn test_encored_pokemon_struggles_not_the_encored_move() {
    let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
    s.use_last_used_move = true;
    s.side_one
        .volatile_statuses
        .insert(PokemonVolatileStatus::ENCORE);
    // encored into a slot other than M0 (Struggle's Choice carries the default index M0)
    s.side_one.last_used_move = LastUsedMove::Move(PokemonMoveIndex::M1);
    let (dmg, recoil, a) = struggle_once(&mut s);
    assert!(
        dmg > 0 && recoil == 41,
        "Struggle's damage and recoil, not Flash Cannon's"
    );
    assert_eq!(a.pp, [0, 0, 0, 0]);
}

/// A choice item does not lock into Struggle (no move becomes disabled).
#[test]
fn test_struggle_does_not_choice_lock() {
    for item in [Items::CHOICEBAND, Items::CHOICESCARF, Items::CHOICESPECS] {
        let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
        s.side_one.get_active().item = item;
        assert_eq!(struggle_once(&mut s).2.disabled, [false; 4], "{:?}", item);
    }
}

/// Both last Pokemon: Struggle KOs the foe and the recoil KOs the user. Showdown awards the Struggle user
/// (its Pokemon fainted last). The engine leaves both at 0 HP; who wins is the environment's
/// simultaneous-wipeout rule (a known, separately tracked discrepancy) - this test pins the HP outcome only.
#[test]
fn test_struggle_double_knockout_hp() {
    let mut s = state(archaludon(), umbreon(), Choices::SPLASH);
    s.side_one.get_active().hp = 10;
    s.side_two.get_active().hp = 1;
    for i in 1..6 {
        s.side_one.pokemon.pkmn[i].hp = 0;
        s.side_two.pokemon.pkmn[i].hp = 0;
    }
    let (_, _, a) = struggle_once(&mut s);
    assert_eq!((a.user_hp, a.foe_hp), (0, 0));
}
