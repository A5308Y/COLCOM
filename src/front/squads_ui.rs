//! Setting up squads during normal play: the Combat tab of the colonist
//! inspector (role, squad, loadout) and the Squads overview window.

use super::input::{Selection, UiState};
use super::sim::Sim;
use super::theme;
use bevy_egui::egui::{self, Align2, RichText};
use colcom::defs::{ItemId, Slot};
use colcom::map::Zone;
use colcom::pawn::{Pawn, PawnId, Role};
use colcom::battle::CELLS_PER_ROUND;
use colcom::squads::{fights_at_range, loadout_worn, style_name, SQUAD_MAX};
use colcom::world::World as SimWorld;


/// A change asked for in the Combat tab, applied after drawing.
pub enum CombatAction {
    Role(Role),
    Loadout(Vec<ItemId>),
    Sidearm(Option<ItemId>),
}

fn label_or_none(kind: Option<ItemId>) -> String {
    kind.map_or("None".to_string(), |k| k.name().to_string())
}

/// One loadout slot: a dropdown of every item that fits it, with how many are in storage.
fn slot_choice(
    ui: &mut egui::Ui,
    w: &SimWorld,
    id: PawnId,
    label: &str,
    current: Option<ItemId>,
    fits: impl Fn(ItemId) -> bool,
) -> Option<Option<ItemId>> {
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.add_sized([70.0, 18.0], egui::Label::new(label));
        egui::ComboBox::from_id_salt(("loadout", id, label)).selected_text(label_or_none(current)).show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), "None").clicked() {
                chosen = Some(None);
            }
            for k in ItemId::all().filter(|&k| fits(k)) {
                let text = format!("{} ({} stored)", k.name(), w.map.count_items(k));
                if ui.selectable_label(current == Some(k), text).clicked() {
                    chosen = Some(Some(k));
                }
            }
        });
    });
    chosen
}

/// How well a colonist fights: their skills, and the chances to hit with the
/// weapon they have (before cover and the like).
fn fighting(ui: &mut egui::Ui, p: &Pawn) {
    use colcom::battle::base_hit_chance;
    use colcom::pawn::Skill;
    theme::section(ui, "Fighting");
    ui.label(format!("Shooting {}   Melee {}", p.skill(Skill::Shooting), p.skill(Skill::Melee)));
    let w = p.main_attack();
    let pct = |c: f32| format!("{:.0}%", c.clamp(0.05, 0.95) * 100.0);
    let text = if w.ranged {
        let far = w.range.floor().max(1.0);
        format!(
            "Hits with {}: {} at 3 cells, {} at {:.0} (its range), before cover",
            p.weapon_name().to_lowercase(),
            pct(base_hit_chance(p, true, 3.0)),
            pct(base_hit_chance(p, true, far)),
            far
        )
    } else {
        format!("Hits with {}: {} of blows", p.weapon_name().to_lowercase(), pct(base_hit_chance(p, false, 1.0)))
    };
    ui.label(text);
    if let Some(side) = p.sidearm {
        let ranged = side.def().weapon.as_ref().is_some_and(|w| w.ranged());
        let chance = if ranged { base_hit_chance(p, true, 3.0) } else { base_hit_chance(p, false, 1.0) };
        let at = if ranged { "at 3 cells" } else { "of blows" };
        ui.label(format!("Sidearm {}: {} {} (drawn when it suits better)", side.name().to_lowercase(), pct(chance), at));
        let quick = format!(
            "An attack with sidearms only costs {} AP instead of {}. Rifles aim badly at someone right next to them; pistols don't.",
            colcom::battle::AP_SIDEARM_ATTACK,
            colcom::battle::AP_ATTACK
        );
        ui.label(RichText::new(quick).small().weak());
    }
    if p.aim_bonus() != 0.0 {
        ui.label(format!("Gear and implants: {:+.0}% to hit", p.aim_bonus() * 100.0));
    }
    let how = "Shots: 55% + 2.5% per Shooting level - 2.5% per cell away; blows: 60% + 2% per Melee level. \
               Hurt eyes or hands lower both; in battle, cover, hunkering down, height and being pinned change it.";
    ui.label(RichText::new(how).small().weak());
}

/// Role, squad and loadout of a colonist.
pub fn combat_tab(ui: &mut egui::Ui, w: &SimWorld, p: &Pawn) -> Vec<CombatAction> {
    let mut actions = Vec::new();
    fighting(ui, p);
    ui.separator();
    theme::section(ui, "When a battle starts");
    let mut role = p.role;
    ui.horizontal(|ui| {
        ui.radio_value(&mut role, Role::Fighter, "Fighter").on_hover_text("Fights as a unit of their own");
        ui.radio_value(&mut role, Role::Civilian, "Civilian").on_hover_text("Shelters in the safe zone");
        ui.radio_value(&mut role, Role::Leader, "Squad leader")
            .on_hover_text("Others can join their squad; needs a decent bedroom of their own");
    });
    let leaders: Vec<&Pawn> = w.pawns.iter().filter(|o| o.role == Role::Leader && o.id != p.id && !o.is_dead()).collect();
    if !leaders.is_empty() {
        ui.horizontal(|ui| {
            let current = match p.role {
                Role::Member(l) => w.pawn(l).map_or("?".to_string(), |l| format!("{}'s squad", l.name)),
                _ => "No squad".to_string(),
            };
            ui.label("Squad:");
            egui::ComboBox::from_id_salt(("squad", p.id)).selected_text(current).show_ui(ui, |ui| {
                for l in leaders {
                    let size = 1 + w.squad_members(l.id).len();
                    let style = style_name(fights_at_range(l));
                    let text = format!("{}'s {} squad ({}/{})", l.name, style, size, SQUAD_MAX);
                    // Only squads that fight the way they do.
                    let fits = fights_at_range(l) == fights_at_range(p);
                    let item = ui.add_enabled(fits, egui::Button::selectable(p.role == Role::Member(l.id), text));
                    let item = item.on_disabled_hover_text(format!(
                        "{} fights {}: give them a {} weapon in the loadout first",
                        p.name,
                        style_name(fights_at_range(p)),
                        style
                    ));
                    if item.clicked() {
                        role = Role::Member(l.id);
                    }
                }
            });
        });
    }
    if role != p.role {
        actions.push(CombatAction::Role(role));
    }
    if p.role == Role::Leader {
        let members: Vec<String> =
            w.squad_members(p.id).iter().filter_map(|&m| w.pawn(m).map(|m| m.name.clone())).collect();
        let text = if members.is_empty() { "No one in the squad yet".to_string() } else { members.join(", ") };
        ui.label(RichText::new(format!("{} squad: {}", capitalised(style_name(fights_at_range(p))), text)).weak());
        if p.poor_quarters {
            let text = "Wants a decent bedroom of their own. Click a bed to give it to them.";
            ui.label(RichText::new(text).color(theme::RED));
        }
    }

    ui.separator();
    theme::section(ui, "Loadout");
    let about = "The gear they use: fetched from storage and kept on (right-click gear to set it too). Empty slots: they take the best they find.";
    ui.label(RichText::new(about).small().weak());
    let style = style_name(fights_at_range(p));
    let rule = match p.role {
        Role::Member(_) => format!("Fights {}, like the whole squad: the weapon must stay {}.", style, style),
        Role::Leader => format!("Fights {}. A weapon of the other kind makes it a squad of that kind.", style),
        _ => format!("Fights {}.", style),
    };
    ui.label(RichText::new(rule).small().weak());
    let weapon = p.loadout.iter().copied().find(|k| k.def().weapon.is_some());
    let utility = p.loadout.iter().copied().find(|k| k.def().utility.is_some());
    // What a choice replaces: the old item for that slot (a suit may fill several).
    let mut change: Option<(Option<ItemId>, Option<ItemId>)> = None;
    if let Some(c) = slot_choice(ui, w, p.id, "Weapon", weapon, |k| k.def().weapon.is_some()) {
        change = Some((weapon, c));
    }
    // A ranged squad's sidearms shoot too; melee squads may take a pistol.
    let sidearm = |k: ItemId| k.def().weapon.as_ref().is_some_and(|w| w.sidearm) && colcom::squads::sidearm_fits_squad(p, k);
    if let Some(c) = slot_choice(ui, w, p.id, "Sidearm", p.loadout_sidearm, sidearm) {
        actions.push(CombatAction::Sidearm(c));
    }
    if let Some(c) = slot_choice(ui, w, p.id, "Utility", utility, |k| k.def().utility.is_some()) {
        change = Some((utility, c));
    }
    for slot in Slot::ALL {
        let fits = |k: ItemId| k.def().apparel.as_ref().is_some_and(|a| a.covers(slot));
        let current = loadout_worn(p, slot);
        if let Some(c) = slot_choice(ui, w, p.id, slot.name(), current, fits) {
            change = Some((current, c));
        }
    }
    if let Some((old, item)) = change {
        // The new item takes out whatever it overlaps (`set_loadout`).
        let mut loadout: Vec<ItemId> = p.loadout.iter().copied().filter(|&k| Some(k) != old).collect();
        loadout.extend(item);
        actions.push(CombatAction::Loadout(loadout));
    }
    ui.horizontal(|ui| {
        if ui.button("Use current gear").clicked() {
            let gear = p.weapon.into_iter().chain(p.apparel.iter().copied()).chain(p.utility.map(|s| s.kind));
            actions.push(CombatAction::Loadout(gear.collect()));
            actions.push(CombatAction::Sidearm(p.sidearm));
        }
        if (!p.loadout.is_empty() || p.loadout_sidearm.is_some()) && ui.button("Clear").clicked() {
            actions.push(CombatAction::Loadout(Vec::new()));
            actions.push(CombatAction::Sidearm(None));
        }
    });
    actions
}

pub fn apply(w: &mut SimWorld, id: PawnId, actions: Vec<CombatAction>) {
    for a in actions {
        match a {
            CombatAction::Role(role) => {
                if let Err(e) = w.set_role(id, role) {
                    w.push_log(format!("Can't do that: {}.", e));
                }
            }
            CombatAction::Loadout(items) => {
                if let Err(e) = w.set_loadout(id, &items) {
                    w.push_log(format!("Can't do that: {}.", e));
                }
            }
            CombatAction::Sidearm(kind) => {
                if let Err(e) = w.set_loadout_sidearm(id, kind) {
                    w.push_log(format!("Can't do that: {}.", e));
                }
            }
        }
    }
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
}

/// Roughly how many rounds a member needs to reach their leader.
fn rounds_away(a: &Pawn, b: &Pawn) -> usize {
    let cells = a.pos.0.abs_diff(b.pos.0) + a.pos.1.abs_diff(b.pos.1) + a.pos.2.abs_diff(b.pos.2);
    cells.saturating_sub(2).div_ceil(CELLS_PER_ROUND)
}

/// Overview of squads, fighters and civilians; clicking a name selects them.
pub fn squads_window(ctx: &egui::Context, sim: &Sim, st: &mut UiState) {
    let w = &sim.world;
    let colonists: Vec<&Pawn> = w.pawns.iter().filter(|p| p.is_living_colonist()).collect();
    let mut select = None;
    let mut open = st.show_squads;
    let name = |ui: &mut egui::Ui, p: &Pawn, extra: String, select: &mut Option<PawnId>| {
        if ui.link(format!("{}{}", p.name, extra)).clicked() {
            *select = Some(p.id);
        }
    };
    theme::FieldWindow::new("Squads")
        .open(&mut open)
        .anchor(Align2::RIGHT_TOP, [-8.0, 92.0])
        .show(ctx, |ui| {
            let leaders: Vec<&&Pawn> = colonists.iter().filter(|p| p.role == Role::Leader).collect();
            if leaders.is_empty() {
                ui.label(RichText::new("No squads: everyone fights on their own.").weak());
            }
            for l in leaders {
                let members: Vec<&&Pawn> = colonists.iter().filter(|p| p.role == Role::Member(l.id)).collect();
                ui.horizontal(|ui| {
                    let style = style_name(fights_at_range(l));
                    name(ui, l, format!(" ({}, {}/{})", style, 1 + members.len(), SQUAD_MAX), &mut select);
                    if l.poor_quarters {
                        ui.label(RichText::new("wants a decent bedroom").small().color(theme::RED));
                    }
                });
                for m in members {
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        let away = match rounds_away(m, l) {
                            0 => " - with them".to_string(),
                            r => format!(" - about {} round{} away", r, if r == 1 { "" } else { "s" }),
                        };
                        name(ui, m, away, &mut select);
                        // Picked up a weapon of the other kind (and has none in the loadout).
                        if fights_at_range(m) != fights_at_range(l) {
                            let text = format!("armed {}: fights alone", style_name(fights_at_range(m)));
                            ui.label(RichText::new(text).small().color(theme::RED));
                        }
                    });
                }
            }
            for (role, title) in [(Role::Fighter, "Fighting alone"), (Role::Civilian, "Civilians")] {
                let group: Vec<&&Pawn> = colonists.iter().filter(|p| p.role == role).collect();
                if group.is_empty() {
                    continue;
                }
                ui.separator();
                theme::section(ui, title);
                ui.horizontal_wrapped(|ui| {
                    for p in group {
                        name(ui, p, String::new(), &mut select);
                    }
                });
            }
            let civilians = colonists.iter().any(|p| p.role == Role::Civilian);
            if civilians && !w.map.zone.contains(&Zone::Safe) {
                ui.label(RichText::new("No safe zone: civilians stay where they are.").color(theme::RED));
            }
            ui.separator();
            ui.label(RichText::new("Set roles and loadouts in a colonist's Combat tab.").small().weak());
        });
    st.show_squads = open;
    if let Some(id) = select {
        st.select(Some(Selection::Pawn(id)));
        st.inspector_tab = super::input::InspectorTab::Combat;
    }
}
