use bevy_app::{PluginGroup, PluginGroupBuilder};
use bevy_window::WindowDescriptor;

/// This plugin group will add all the default plugins:
/// * [`LogPlugin`](bevy_log::LogPlugin)
/// * [`CorePlugin`](bevy_core::CorePlugin)
/// * [`TimePlugin`](bevy_time::TimePlugin)
/// * [`TransformPlugin`](bevy_transform::TransformPlugin)
/// * [`HierarchyPlugin`](bevy_hierarchy::HierarchyPlugin)
/// * [`DiagnosticsPlugin`](bevy_diagnostic::DiagnosticsPlugin)
/// * [`InputPlugin`](bevy_input::InputPlugin)
/// * [`WindowPlugin`](bevy_window::WindowPlugin)
/// * [`AssetPlugin`](bevy_asset::AssetPlugin)
/// * [`ScenePlugin`](bevy_scene::ScenePlugin)
/// * [`RenderPlugin`](bevy_render::RenderPlugin) - with feature `bevy_render`
/// * [`SpritePlugin`](bevy_sprite::SpritePlugin) - with feature `bevy_sprite`
/// * [`PbrPlugin`](bevy_pbr::PbrPlugin) - with feature `bevy_pbr`
/// * [`UiPlugin`](bevy_ui::UiPlugin) - with feature `bevy_ui`
/// * [`TextPlugin`](bevy_text::TextPlugin) - with feature `bevy_text`
/// * [`AudioPlugin`](bevy_audio::AudioPlugin) - with feature `bevy_audio`
/// * [`GilrsPlugin`](bevy_gilrs::GilrsPlugin) - with feature `bevy_gilrs`
/// * [`GltfPlugin`](bevy_gltf::GltfPlugin) - with feature `bevy_gltf`
/// * [`WinitPlugin`](bevy_winit::WinitPlugin) - with feature `bevy_winit`
/// * [`XrPlugin`] - with feature `bevy_xr`
/// * [`OpenXrPlugin`] - with feature `bevy_openxr`
///
/// See also [`MinimalPlugins`] for a slimmed down option
pub struct DefaultPlugins;

impl PluginGroup for DefaultPlugins {
    fn build(self) -> PluginGroupBuilder {
        let mut group = PluginGroupBuilder::start::<Self>()
            .add(bevy_log::LogPlugin::default())
            .add(bevy_core::CorePlugin::default())
            .add(bevy_time::TimePlugin::default())
            .add(bevy_transform::TransformPlugin::default())
            .add(bevy_hierarchy::HierarchyPlugin::default())
            .add(bevy_diagnostic::DiagnosticsPlugin::default())
            .add(bevy_input::InputPlugin::default());
        #[cfg(not(feature = "bevy_xr"))]
        {
            group = group.add(bevy_window::WindowPlugin::default());
        }
        #[cfg(feature = "bevy_xr")]
        {
            group = group.add(bevy_window::WindowPlugin {
                window: WindowDescriptor::default(),
                add_primary_window: cfg!(feature = "bevy_winit"),
                exit_on_all_closed: cfg!(feature = "bevy_winit"),
                close_when_requested: cfg!(feature = "bevy_winit"),
            });
        }

        #[cfg(feature = "bevy_asset")]
        {
            group = group.add(bevy_asset::AssetPlugin::default());
        }

        #[cfg(feature = "debug_asset_server")]
        {
            group = group.add(bevy_asset::debug_asset_server::DebugAssetServerPlugin::default());
        }

        #[cfg(feature = "bevy_scene")]
        {
            group = group.add(bevy_scene::ScenePlugin::default());
        }

        #[cfg(feature = "bevy_winit")]
        {
            group = group.add(bevy_winit::WinitPlugin::default());
        }

        //  needs to be before render plugin and after bevy_winit for now
        #[cfg(feature = "bevy_openxr")]
        {
            group = group.add(bevy_openxr::OpenXrPlugin::default());
        }

        #[cfg(feature = "bevy_render")]
        {
            group = group
                .add(bevy_render::RenderPlugin::default())
                // NOTE: Load this after renderer initialization so that it knows about the supported
                // compressed texture formats
                .add(bevy_render::texture::ImagePlugin::default());
        }

        #[cfg(feature = "bevy_core_pipeline")]
        {
            group = group.add(bevy_core_pipeline::CorePipelinePlugin::default());
        }

        //  must be after core pipeline
        #[cfg(feature = "bevy_openxr")]
        {
            group = group.add(bevy_openxr::camera::xrcameraplugin::XrCameraPlugin::default());
        }

        #[cfg(feature = "bevy_sprite")]
        {
            group = group.add(bevy_sprite::SpritePlugin::default());
        }

        #[cfg(feature = "bevy_text")]
        {
            group = group.add(bevy_text::TextPlugin::default());
        }

        #[cfg(all(feature = "bevy_ui", not(feature = "bevy_xr")))]
        {
            group = group.add(bevy_ui::UiPlugin::default());
        }

        #[cfg(feature = "bevy_pbr")]
        {
            group = group.add(bevy_pbr::PbrPlugin::default());
        }

        // NOTE: Load this after renderer initialization so that it knows about the supported
        // compressed texture formats
        #[cfg(feature = "bevy_gltf")]
        {
            group = group.add(bevy_gltf::GltfPlugin::default());
        }

        #[cfg(feature = "bevy_audio")]
        {
            group = group.add(bevy_audio::AudioPlugin::default());
        }

        #[cfg(feature = "bevy_gilrs")]
        {
            group = group.add(bevy_gilrs::GilrsPlugin::default());
        }

        #[cfg(feature = "bevy_xr")]
        {
            group = group.add(bevy_xr::XrPlugin::default());
        }

        #[cfg(feature = "bevy_animation")]
        {
            group = group.add(bevy_animation::AnimationPlugin::default());
        }

        group
    }
}

/// Minimal plugin group that will add the following plugins:
/// * [`CorePlugin`](bevy_core::CorePlugin)
/// * [`TimePlugin`](bevy_time::TimePlugin)
/// * [`ScheduleRunnerPlugin`](bevy_app::ScheduleRunnerPlugin)
///
/// See also [`DefaultPlugins`] for a more complete set of plugins
pub struct MinimalPlugins;

impl PluginGroup for MinimalPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(bevy_core::CorePlugin::default())
            .add(bevy_time::TimePlugin::default())
            .add(bevy_app::ScheduleRunnerPlugin::default())
    }
}
