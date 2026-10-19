//! The on-screen keyboard (ours; doc 31 "Extras and cheat codes"): drawn while retail FE 46 (text entry) is on top,
//! where retail showed the Xbox system keyboard. The state is `extras_host::TextEntryView` (the stage owns the input);
//! this only draws it: the title, the text so far, the key grid with the highlighted key, and the button help.
use crate::extras_host::TextEntryView;
use bevy::prelude::*;
use skate_core::text_entry::Key;

#[derive(Component)]
struct Root;

pub(crate) struct TextEntryUiPlugin;
impl Plugin for TextEntryUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TextEntryView>().add_systems(Update, draw.run_if(resource_changed::<TextEntryView>));
    }
}

const PANEL: Color = Color::srgba(0.025, 0.035, 0.048, 0.97);
const KEY: Color = Color::srgba(0.09, 0.11, 0.15, 1.0);
const KEY_LIT: Color = Color::srgba(1.0, 0.78, 0.16, 1.0);

fn label(key: Key, shown: Option<char>) -> String {
    match key {
        Key::Char(_) => shown.map(String::from).unwrap_or_default(),
        Key::Space => "Space".into(),
        Key::Backspace => "Delete".into(),
        Key::Shift => "Shift".into(),
        Key::Done => "Done".into(),
        Key::Cancel => "Cancel".into(),
    }
}

fn draw(mut commands: Commands, view: Res<TextEntryView>, roots: Query<Entity, With<Root>>) {
    for root in &roots {
        commands.entity(root).despawn();
    }
    let Some(keyboard) = view.keyboard.as_ref().filter(|_| !view.hidden) else { return };
    let text = |s: String, size: f32, color: Color| (Text::new(s), TextFont { font_size: size, ..default() }, TextColor(color));
    commands
        .spawn((
            Root,
            GlobalZIndex(30),
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                top: percent(50),
                margin: UiRect { left: px(-330), top: px(-210), ..default() },
                width: px(660),
                padding: UiRect::all(px(22)),
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                border_radius: BorderRadius::all(px(16)),
                ..default()
            },
            BackgroundColor(PANEL),
        ))
        .with_children(|p| {
            p.spawn(text(view.title.clone(), 24.0, Color::WHITE));
            // The text so far, with a caret.
            p.spawn((
                Node { padding: UiRect::axes(px(12), px(8)), border_radius: BorderRadius::all(px(8)), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            ))
            .with_children(|f| {
                f.spawn(text(format!("{}_", keyboard.text), 22.0, Color::WHITE));
            });
            for (r, row) in view.layout.rows.iter().enumerate() {
                p.spawn(Node { column_gap: px(6), justify_content: JustifyContent::Center, ..default() }).with_children(|line| {
                    for (c, key) in row.iter().enumerate() {
                        let lit = r == keyboard.row && c == keyboard.col.min(row.len().saturating_sub(1));
                        let wide = !matches!(key, Key::Char(_));
                        line.spawn((
                            Node {
                                width: px(if wide { 104.0 } else { 52.0 }),
                                height: px(44),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border_radius: BorderRadius::all(px(8)),
                                ..default()
                            },
                            BackgroundColor(if lit { KEY_LIT } else { KEY }),
                        ))
                        .with_children(|k| {
                            let ink = if lit { Color::srgb(0.08, 0.07, 0.04) } else { Color::WHITE };
                            k.spawn(text(label(*key, keyboard.shown(*key)), 20.0, ink));
                        });
                    }
                });
            }
            p.spawn(text("A type   X delete   Y shift   Start done   B cancel   (or type on the keyboard)".into(), 14.0, Color::srgb(0.6, 0.66, 0.72)));
        });
}
