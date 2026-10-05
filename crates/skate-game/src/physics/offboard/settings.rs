//! Stock Biped settings82D7B630 and metric queries82D7AFD8/82D16680.
mod board;
mod curves;
mod metrics;
#[cfg(test)]
mod tests;
use skate_core::{
    player::offboard::{air_launch, controller, movement_intent, movement_velocity},
    point_graph::PointGraph,
};
use skate_data::{animation_metadata::AnimationMetadata, collections::Collections};

/// Object-move curves of the inputlistener collection read by 8259C4B0
/// (`skate_core::input::offboard_intentions::produce_object_move`).
pub(crate) fn load_object_move_curves(
    data: &Collections,
) -> Result<skate_core::input::offboard_intentions::ObjectMoveCurves, String> {
    Ok(skate_core::input::offboard_intentions::ObjectMoveCurves {
        x_gain: curves::load::<8>(data, "inputlistener", "Hash_1A1A7AC37A72DF87")?.0,
        z_gain: curves::load::<8>(data, "inputlistener", "Hash_05BA8B52C23B3481")?.0,
        rotation: curves::load::<16>(data, "inputlistener", "Hash_9ADFC2E222938C1E")?.0,
    })
}

pub(crate) struct Settings {
    pub controller: controller::Settings,
    pub board: skate_core::player::offboard::ground_sync::BoardSettings,
    pub metrics: [Option<controller::ClipMetric>; 3],
    ///82D310F8 full key DF759B46440F16E9.
    pub movement_vs_stick_angle: PointGraph<8>,
    ///82D310F8 full key2DD95B399BAE313E.
    pub turn_vs_stick_angle: PointGraph<8>,
    pub air_launch: air_launch::Settings,
}
impl Settings {
    /// Share the same native ABIN metadata loaded for SkaterAnimation. Required
    /// stock fields/banks/clips fail loading; an absent matching attribute is None.
    pub(crate) fn load(data: &Collections, metadata: &AnimationMetadata) -> Result<Self, String> {
        let biped = |name| curves::load::<8>(data, "physics_biped", name);
        let (sprint_blend, bounds) = biped("Hash_6B93C51256A30FB4")?;
        Ok(Self {
            controller: controller::Settings {
                movement_intent: movement_intent::Settings {
                    sprint_speed: curves::load::<4>(
                        data,
                        "physics_biped",
                        "Hash_7209DCFDF3015EBF",
                    )?
                    .0,
                    normal_speed: curves::load::<16>(data, "physics_biped", "SpeedVsInput")?.0,
                    sprint_blend,
                    sprint_time_cap: bounds[2],
                    slide_steering: biped("AutoTurnVsAngle")?.0,
                },
                movement_velocity: movement_velocity::Settings {
                    slope_speed_scalar: biped("Hash_31309236050A8F09")?.0,
                    slope_mode_speed: biped("Hash_CE45C724B30F9134")?.0,
                    turn_vs_speed: biped("TurnVsSpeed")?.0,
                    turn_delta_vs_speed: biped("TurnDeltaVsSpeed")?.0,
                },
                slide_vs_slope: biped("SlideVsSlope")?.0,
                slide_vs_speed: biped("SlideVsSpeed")?.0,
            },
            metrics: metrics::load(metadata)?,
            board: board::load(data)?,
            movement_vs_stick_angle: curves::load::<8>(
                data,
                "physics_state_offboard",
                "Hash_DF759B46440F16E9",
            )?
            .0,
            turn_vs_stick_angle: curves::load::<8>(
                data,
                "physics_state_offboard",
                "TurnVsStickAngle",
            )?
            .0,
            air_launch: air_launch::Settings {
                jump_speed_scalar: data.float("physics_biped", "default", "JumpSpeedScalar")?,
                jump_height: data.float("physics_biped", "default", "JumpHeight")?,
            },
        })
    }

    pub(crate) fn into_controller_parts(
        self,
    ) -> (
        controller::Settings,
        [Option<controller::ClipMetric>; 3],
        PointGraph<8>,
        PointGraph<8>,
        air_launch::Settings,
    ) {
        (
            self.controller,
            self.metrics,
            self.movement_vs_stick_angle,
            self.turn_vs_stick_angle,
            self.air_launch,
        )
    }
}
