//! The build menu: a full-height panel docked on the left with every order,
//! zone and buildable, grouped, searchable, with costs and details.

use super::input::{ToolEntry, Tool, UiState, TOOLS};
use super::sim::Sim;
use bevy_egui::egui::{self, Color32, RichText};
use my_world::defs::{BuildDef, Special};
use my_world::world::Order;

const OPEN_WIDTH: f32 = 250.0;
const CLOSED_WIDTH: f32 = 30.0;

/// Draws the dock and returns its width, so other windows can keep clear of it.
pub fn draw(ctx: &egui::Context, sim: &Sim, st: &mut UiState) -> f32 {
    let mut root = egui::Ui::new(
        ctx.clone(),
        "build_dock_root".into(),
        egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(ctx.viewport_rect()),
    );
    if !st.show_build {
        let panel = egui::Panel::left("build_dock_closed").resizable(false).exact_size(CLOSED_WIDTH);
        return panel
            .show(&mut root, |ui| {
                if ui.button("»").on_hover_text("Show the build menu (`)").clicked() {
                    st.show_build = true;
                }
            })
            .response
            .rect
            .width();
    }
    let totals = sim.world.item_totals();
    let panel = egui::Panel::left("build_dock").resizable(true).default_size(OPEN_WIDTH).size_range(200.0..=380.0);
    panel
        .show(&mut root, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Build");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("«").on_hover_text("Hide the build menu (`)").clicked() {
                        st.show_build = false;
                    }
                });
            });
            let search = ui.add(
                egui::TextEdit::singleline(&mut st.build_filter)
                    .hint_text("Search (fuzzy, Enter picks the best)")
                    .desired_width(f32::INFINITY),
            );
            ui.separator();

            // Short note about the current tool, kept at the bottom.
            egui::Panel::bottom("build_dock_hint").resizable(false).show(ui, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(current_hint(st.tool)).small().weak());
            });

            let query = st.build_filter.trim().to_string();
            if query.is_empty() {
                grouped(ui, st, &totals);
                return;
            }
            let results = search_tools(&query);
            if search.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if let Some((e, _)) = results.first() {
                    st.tool = e.tool;
                    st.drag_start = None;
                    st.build_filter.clear();
                }
            }
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                if results.is_empty() {
                    ui.label(RichText::new("Nothing matches").weak());
                }
                for (e, hits) in &results {
                    entry(ui, e, st, &totals, Some(hits));
                }
            });
        })
        .response
        .rect
        .width()
}

/// Every tool, in collapsible groups.
fn grouped(ui: &mut egui::Ui, st: &mut UiState, totals: &[u32]) {
    let mut groups: Vec<&str> = Vec::new();
    for e in TOOLS.iter() {
        if !groups.contains(&e.group.as_str()) {
            groups.push(&e.group);
        }
    }
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for group in groups {
            let header = egui::CollapsingHeader::new(RichText::new(group).strong())
                .id_salt(("build_group", group))
                .default_open(true);
            header.show(ui, |ui| {
                for e in TOOLS.iter().filter(|e| e.group == group) {
                    entry(ui, e, st, totals, None);
                }
            });
        }
    });
}

/// Tools matching `query`, best first, with the matched letters of their names.
fn search_tools(query: &str) -> Vec<(&'static ToolEntry, Vec<usize>)> {
    let mut found: Vec<(i32, usize, &ToolEntry, Vec<usize>)> = Vec::new();
    for (k, e) in TOOLS.iter().enumerate() {
        // The name counts most; its group, cost and what it's for help find it too.
        let by_name = fuzzy(query, &e.name);
        let by_words = fuzzy(query, &keywords(e)).map(|(score, _)| (score - 12, Vec::new()));
        let best = match (by_name, by_words) {
            (Some(a), Some(b)) => Some(if a.0 >= b.0 { a } else { b }),
            (a, b) => a.or(b),
        };
        if let Some((score, hits)) = best {
            found.push((score, k, e, hits));
        }
    }
    // Ties keep the menu's order.
    found.sort_by_key(|&(score, k, _, _)| (-score, k));
    found.into_iter().map(|(_, _, e, hits)| (e, hits)).collect()
}

/// Other words a tool can be found by.
fn keywords(e: &ToolEntry) -> String {
    let mut words = vec![e.group.clone()];
    if let Some(d) = build_def(e.tool) {
        words.push(d.cost.item.name().to_string());
        words.extend(d.recipes.iter().map(|r| r.name().to_string()));
        if d.light.is_some() {
            words.push("light".into());
        }
        if d.storage.is_some() {
            words.push("storage".into());
        }
    }
    words.join(" ")
}

/// Fuzzy match: the letters of `query` in order somewhere in `text`, case
/// ignored, spaces in the query ignored. Scores favour matches at the start
/// of words and runs of letters in a row, and penalise gaps. Returns the
/// score and the matched character positions in `text`.
pub fn fuzzy(query: &str, text: &str) -> Option<(i32, Vec<usize>)> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    let t: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    if q.is_empty() || q.len() > t.len() {
        return None;
    }
    let word_start = |j: usize| j == 0 || !t[j - 1].is_alphanumeric();
    const NONE: i32 = i32::MIN / 2;
    // best[i][j]: best score with query letter i matched at text position j; from[i][j]: previous position.
    let mut best = vec![vec![NONE; t.len()]; q.len()];
    let mut from = vec![vec![0usize; t.len()]; q.len()];
    for i in 0..q.len() {
        for j in i..t.len() {
            if t[j] != q[i] {
                continue;
            }
            let here = 10 + if word_start(j) { 8 } else { 0 };
            if i == 0 {
                // A little worse the further in the first letter is.
                best[i][j] = here - (j as i32).min(5);
                continue;
            }
            for p in i - 1..j {
                if best[i - 1][p] == NONE {
                    continue;
                }
                let step = if p + 1 == j { 6 } else { -((j - p - 1) as i32).min(6) };
                let score = best[i - 1][p] + here + step;
                if score > best[i][j] {
                    best[i][j] = score;
                    from[i][j] = p;
                }
            }
        }
    }
    let last = q.len() - 1;
    let (mut j, score) = (0..t.len()).map(|j| (j, best[last][j])).max_by_key(|&(_, s)| s)?;
    if score == NONE {
        return None;
    }
    // Shorter names win ties: "Door" before "Stone door".
    let score = score - (t.len() as i32 / 8);
    let mut hits = vec![j];
    for i in (1..q.len()).rev() {
        j = from[i][j];
        hits.push(j);
    }
    hits.reverse();
    Some((score, hits))
}

/// A name with the matched letters picked out.
fn highlighted(ui: &egui::Ui, name: &str, hits: &[usize], strong: Color32) -> egui::text::LayoutJob {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let normal = ui.visuals().text_color();
    let mut job = egui::text::LayoutJob::default();
    for (k, c) in name.chars().enumerate() {
        let hit = hits.contains(&k);
        let format = egui::TextFormat {
            font_id: font.clone(),
            color: if hit { strong } else { normal },
            underline: if hit { egui::Stroke::new(1.0, strong) } else { egui::Stroke::NONE },
            ..Default::default()
        };
        job.append(&c.to_string(), 0.0, format);
    }
    job
}

/// One tool: name and shortcut, cost underneath, details on hover. In
/// search results (`hits`: matched letters) the name is highlighted and its
/// group shown.
fn entry(ui: &mut egui::Ui, e: &ToolEntry, st: &mut UiState, totals: &[u32], hits: Option<&Vec<usize>>) {
    let selected = st.tool == e.tool;
    let def = build_def(e.tool);
    let cost = def.map(|d| {
        let c = &d.cost;
        let have = totals[c.item.index()];
        let text = format!("{} {}", c.count, c.item.name().to_lowercase());
        (text, have >= c.count)
    });
    let visuals = ui.visuals().clone();
    let fill = if selected { visuals.selection.bg_fill } else { Color32::TRANSPARENT };
    let frame = egui::Frame::new().fill(fill).corner_radius(4.0).inner_margin(egui::Margin::symmetric(6, 3));
    let response = frame
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                match hits {
                    Some(hits) => {
                        let job = highlighted(ui, &e.name, hits, Color32::from_rgb(250, 200, 90));
                        ui.label(job);
                    }
                    None => {
                        let name = RichText::new(&e.name);
                        ui.label(if selected { name.strong().color(visuals.selection.stroke.color) } else { name });
                    }
                }
                if let Some((key, _)) = &e.key {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(key).monospace().small().weak());
                    });
                }
            });
            ui.horizontal(|ui| {
                if let Some((text, affordable)) = &cost {
                    let color = if *affordable { visuals.weak_text_color() } else { Color32::from_rgb(230, 110, 90) };
                    ui.label(RichText::new(text).small().color(color));
                }
                if hits.is_some() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(&e.group).small().weak());
                    });
                }
            });
        })
        .response
        .interact(egui::Sense::click());
    if response.hovered() && !selected {
        ui.painter().rect_stroke(
            response.rect,
            4.0,
            egui::Stroke::new(1.0, visuals.widgets.hovered.bg_stroke.color),
            egui::StrokeKind::Inside,
        );
    }
    let response = response.on_hover_ui(|ui| details(ui, e, def, totals));
    if response.clicked() {
        st.tool = e.tool;
        st.drag_start = None;
    }
}

fn build_def(tool: Tool) -> Option<&'static BuildDef> {
    match tool {
        Tool::Paint(Order::Build(kind)) => Some(kind.def()),
        _ => None,
    }
}

/// Tooltip: what a tool does, or everything worth knowing about a buildable.
fn details(ui: &mut egui::Ui, e: &ToolEntry, def: Option<&BuildDef>, totals: &[u32]) {
    ui.set_max_width(280.0);
    ui.label(RichText::new(&e.name).strong());
    let Some(d) = def else {
        ui.label(e.hint);
        return;
    };
    let c = &d.cost;
    ui.label(format!(
        "{} {} ({} in the colony), {:.0} s of work",
        c.count,
        c.item.name().to_lowercase(),
        totals[c.item.index()],
        d.work_time
    ));
    let mut facts: Vec<String> = Vec::new();
    match d.special {
        Special::Floor => {
            facts.push("A floor slab for upper storeys: build it in the air, held up by walls or ground within 3 cells".into());
            facts.push("Roofs come by themselves: builders put one over every room you enclose".into());
        }
        Special::Roof => facts.push("Put over enclosed rooms by themselves".into()),
        Special::Stairs => facts.push("Connects this level with the one above; walk on from the low end".into()),
        Special::Bed => facts.push("Colonists sleep and recover here".into()),
        Special::Door => facts.push("Lets colonists through but closes the room".into()),
        Special::Table => facts.push("Colonists eat here, sitting on a chair beside it".into()),
        Special::Seat => facts.push("Put it beside a table to eat at".into()),
        Special::Grave => facts.push("Dug into soil; haulers bury a dead colonist in it".into()),
        Special::None => {}
    }
    if !d.passable && d.special == Special::None {
        facts.push("Blocks the way".into());
    }
    if d.blocks_room && d.special != Special::Door {
        facts.push("Encloses rooms".into());
    }
    if d.holds_floors {
        facts.push("Holds up floors above".into());
    }
    if d.daylight {
        facts.push("Lets daylight into the room".into());
    }
    if d.light.is_some() {
        facts.push("Lights up its surroundings".into());
    }
    if let Some(s) = &d.storage {
        facts.push(format!("Storage by itself: holds {} of an item; click it to choose what goes on it", s.stack_limit));
    }
    if d.beauty != 0.0 {
        facts.push(format!("Beauty {:+}", d.beauty));
    }
    if !d.recipes.is_empty() {
        let makes: Vec<&str> = d.recipes.iter().map(|r| r.name()).collect();
        facts.push(format!("Workbench: {}", makes.join(", ")));
    }
    for f in facts {
        ui.label(RichText::new(format!("- {}", f)).small());
    }
}

fn current_hint(tool: Tool) -> String {
    let name = TOOLS.iter().find(|e| e.tool == tool).map_or("", |e| e.name.as_str());
    match tool {
        Tool::Select => "Click colonists or cells to inspect them.".into(),
        Tool::Paint(Order::Build(_)) => format!("{}: left-drag to place blueprints. Right-click to stop.", name),
        Tool::Paint(_) => format!("{}: left-drag over an area. Right-click to stop.", name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn top(query: &str) -> String {
        search_tools(query).first().map(|(e, _)| e.name.clone()).unwrap_or_default()
    }

    #[test]
    fn letters_in_order_with_gaps_match() {
        assert!(fuzzy("stct", "Stonecutter's table").is_some());
        assert!(fuzzy("wl", "Wall").is_some());
        assert!(fuzzy("lw", "Wall").is_none(), "order matters");
        assert!(fuzzy("STOVE", "stove").is_some(), "case doesn't");
        assert_eq!(fuzzy("cb", "Crafting bench").unwrap().1, vec![0, 9], "word starts preferred");
    }

    #[test]
    fn the_likeliest_tool_comes_first() {
        assert_eq!(top("door"), "Door", "shorter names first");
        assert_eq!(top("chr"), "Chair");
        assert_eq!(top("craft b"), "Crafting bench");
        assert_eq!(top("stcut"), "Stonecutter's table");
        assert_eq!(top("grow"), "Grow zone");
        // Found through what they're for, not just their names.
        assert!(search_tools("meal").iter().any(|(e, _)| e.name == "Stove"));
        assert!(search_tools("zzzz").is_empty());
    }
}
