pub mod camera;
mod conversion;
mod utils;
#[cfg(feature = "winit_loop")]
mod winit;

use bevy_log::error;
#[cfg(feature = "winit_loop")]
use bevy_winit::WinitSettings;
use conversion::*;
mod interaction;
mod presentation;
mod swapchain;
use swapchain::*;
mod setup;

use ash::vk;
use ash::vk::Handle;

use bevy_render::{
    camera::{CameraPlugin, CameraProjection, CameraProjectionPlugin, ManualTextureViews},
    prelude::Msaa,
    renderer::{self, RenderInstance},
    settings::WgpuSettings,
};

use bevy_utils::Uuid;
pub use interaction::*;

#[cfg(feature = "winit_loop")]
use ::winit::event_loop::EventLoop;
use bevy_app::{App, AppExit, Plugin};
use bevy_ecs::{
    event::{Events, ManualEventReader},
    system::Resource,
};
use bevy_xr::{
    presentation::{XrEnvironmentBlendMode, XrGraphicsContext, XrInteractionMode},
    XrActionDescriptor, XrActionSet, XrActionType, XrProfileDescriptor, XrProfiles, XrSessionMode,
    XrSystem, XrTrackingSource, XrVibrationEvent, XrVisibilityState,
};
use openxr::{self as xr, sys};
use parking_lot::RwLock;
use presentation::GraphicsContextHandles;
use serde::{Deserialize, Serialize};
use xr::{ActiveActionSet, ViewStateFlags};

use std::{
    error::Error,
    ffi::{c_char, c_void},
    ops::Deref,
    sync::Arc,
    thread,
    time::Duration,
};
use wgpu::{Backends, TextureUsages, TextureViewDescriptor};
use wgpu_hal::TextureUses;

pub use crate::camera::XrPawn;
use crate::camera::XrViews;

// The form-factor is selected at plugin-creation-time and cannot be changed anymore for the entire
// lifetime of the app. This will restrict which XrSessionMode can be selected.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum OpenXrFormFactor {
    HeadMountedDisplay,
    Handheld,
}

enum SessionBackend {
    Vulkan(xr::Session<xr::Vulkan>),
    #[cfg(all(windows, feature = "wip"))]
    D3D11(xr::Session<xr::D3D11>),
}

enum FrameStream {
    Vulkan(xr::FrameStream<xr::Vulkan>),
    #[cfg(all(windows, feature = "wip"))]
    D3D11(xr::FrameStream<xr::D3D11>),
}

#[derive(Clone, Resource)]
pub struct OpenXrSession {
    inner: Option<xr::Session<xr::AnyGraphics>>,
    _wgpu_device: Arc<wgpu::Device>,
}

impl Deref for OpenXrSession {
    type Target = xr::Session<xr::AnyGraphics>;

    fn deref(&self) -> &Self::Target {
        self.inner.as_ref().unwrap()
    }
}

impl Drop for OpenXrSession {
    fn drop(&mut self) {
        // Drop OpenXR session before wgpu::Device.
        self.inner.take();
    }
}

#[derive(Debug)]
pub enum OpenXrError {
    #[cfg(any(target_os = "android", target_os = "linux"))]
    Loader(xr::LoadError),
    InstanceCreation(sys::Result),
    UnsupportedFormFactor,
    UnavailableFormFactor,
    GraphicsCreation(Box<dyn Error>),
    SwapchainCreation(sys::Result),
}

fn selected_extensions(entry: &xr::Entry) -> xr::ExtensionSet {
    let available = entry.enumerate_extensions().unwrap();

    let mut exts = xr::ExtensionSet::default();
    // Complete list: https://www.khronos.org/registry/OpenXR/specs/1.0/html/xrspec.html#extension-appendices-list
    exts.khr_composition_layer_depth = available.khr_composition_layer_depth;
    // todo: set depth layer
    exts.khr_vulkan_enable = available.khr_vulkan_enable;
    //  required for how we use openxr library
    exts.khr_vulkan_enable2 = true;
    if cfg!(debug_assertions) {
        exts.ext_debug_utils = available.ext_debug_utils;
    }
    exts.ext_eye_gaze_interaction = available.ext_eye_gaze_interaction;
    // todo: implement eye tracking
    exts.ext_hand_tracking = available.ext_hand_tracking;
    exts.ext_hp_mixed_reality_controller = available.ext_hp_mixed_reality_controller;
    exts.ext_performance_settings = available.ext_performance_settings;
    // todo: implement performance API
    exts.ext_samsung_odyssey_controller = available.ext_samsung_odyssey_controller;
    exts.ext_thermal_query = available.ext_thermal_query;
    // todo: implement thermal API
    exts.fb_color_space = available.fb_color_space;
    // todo: implement color space API
    exts.fb_display_refresh_rate = available.fb_display_refresh_rate;
    // todo: implement refresh rate API
    exts.htc_vive_cosmos_controller_interaction = available.htc_vive_cosmos_controller_interaction;
    exts.huawei_controller_interaction = available.huawei_controller_interaction;
    exts.msft_hand_interaction = available.msft_hand_interaction;
    // exts.msft_scene_unserstanding = available.msft_scene_unserstanding -> not available in openxrs
    // todo: implement scene understanding API
    // exts.msft_scene_unserstanding_serialization = available.msft_scene_unserstanding_serialization -> not available in openxrs
    // todo: implement scene serialization
    exts.msft_secondary_view_configuration = available.msft_secondary_view_configuration;
    // todo: implement secondary view. This requires integration with winit.
    exts.msft_spatial_anchor = available.msft_spatial_anchor;
    // todo: implement spatial anchors API
    exts.varjo_quad_views = available.varjo_quad_views;

    #[cfg(target_os = "android")]
    {
        exts.khr_android_create_instance = available.khr_android_create_instance;
        exts.khr_android_thread_settings = available.khr_android_thread_settings;
        exts.ext_debug_utils = true;
        // todo: set APPLICATION_MAIN and RENDER_MAIN threads
    }
    #[cfg(windows)]
    {
        exts.khr_d3d11_enable = available.khr_d3d11_enable;
    }
    bevy_log::info!("selected xr extensions: \n{:#?}", exts);

    exts
}

#[derive(Resource)]
pub struct XrInstanceRes(pub xr::Instance);

#[derive(Resource)]
pub struct OpenXrContext {
    instance: xr::Instance,
    form_factor: xr::FormFactor,
    system: xr::SystemId,
    // Note: the lifecycle of graphics handles is managed by wgpu objects
    graphics_handles: GraphicsContextHandles,
    wgpu_device: Arc<wgpu::Device>,
    graphics_context: Option<XrGraphicsContext>,
}

//  This function is a hack to convert from extern "C" to extern "system".
#[cfg(feature = "simulator")]
unsafe extern "system" fn sim_get_instance_proc_addr(
    _instance: xr::sys::Instance,
    name: *const c_char,
    function: *mut Option<xr::sys::pfn::VoidFunction>,
) -> xr::sys::Result {
    //  The simulator doesn't currently use `instance` argument
    xr::sys::Result::from_raw(bevy_openxr_simulator::get_instance_proc_addr(
        std::ptr::null_mut() as _,
        name,
        function as _,
    ))
}

impl OpenXrContext {
    fn new(form_factor: OpenXrFormFactor) -> Result<Self, OpenXrError> {
        let entry = {
            #[cfg(feature = "simulator")]
            {
                unsafe { xr::Entry::from_get_instance_proc_addr(sim_get_instance_proc_addr) }
                    .unwrap()
            }
            #[cfg(not(feature = "simulator"))]
            {
                #[cfg(any(target_os = "android", target_os = "linux"))]
                unsafe {
                    xr::Entry::load().map_err(OpenXrError::Loader)?
                }
                #[cfg(not(any(target_os = "android", target_os = "linux")))]
                xr::Entry::linked()
            }
        };

        #[cfg(target_os = "android")]
        entry.initialize_android_loader().unwrap();

        let extensions = selected_extensions(&entry);

        let instance = entry
            .create_instance(
                &xr::ApplicationInfo {
                    application_name: "Bevy App",
                    application_version: 0,
                    engine_name: "Bevy Engine",
                    engine_version: 0,
                },
                &extensions,
                &[], // todo: add debug layer
            )
            .map_err(OpenXrError::InstanceCreation)?;

        let form_factor = match form_factor {
            OpenXrFormFactor::HeadMountedDisplay => xr::FormFactor::HEAD_MOUNTED_DISPLAY,
            OpenXrFormFactor::Handheld => xr::FormFactor::HEAD_MOUNTED_DISPLAY,
        };

        let system = instance.system(form_factor).map_err(|e| match e {
            sys::Result::ERROR_FORM_FACTOR_UNSUPPORTED => OpenXrError::UnsupportedFormFactor,
            sys::Result::ERROR_FORM_FACTOR_UNAVAILABLE => OpenXrError::UnavailableFormFactor,
            e => panic!("{}", e), // should never happen
        })?;

        let (graphics_handles, graphics_context) =
            presentation::create_graphics_context(&instance, system)
                .map_err(OpenXrError::GraphicsCreation)?;

        Ok(Self {
            instance,
            form_factor,
            system,
            graphics_handles,
            wgpu_device: graphics_context.device.clone(),
            graphics_context: Some(graphics_context),
        })
    }
}

fn get_system_info(
    instance: &xr::Instance,
    system: xr::SystemId,
    mode: XrSessionMode,
) -> Option<(xr::ViewConfigurationType, xr::EnvironmentBlendMode)> {
    let view_type = match mode {
        XrSessionMode::ImmersiveVR | XrSessionMode::ImmersiveAR => {
            if instance.exts().varjo_quad_views.is_some() {
                xr::ViewConfigurationType::PRIMARY_QUAD_VARJO
            } else {
                xr::ViewConfigurationType::PRIMARY_STEREO
            }
        }
        XrSessionMode::InlineVR | XrSessionMode::InlineAR => {
            xr::ViewConfigurationType::PRIMARY_MONO
        }
    };

    let blend_modes = match instance.enumerate_environment_blend_modes(system, view_type) {
        Ok(blend_modes) => blend_modes,
        _ => return None,
    };

    let blend_mode = match mode {
        XrSessionMode::ImmersiveVR | XrSessionMode::InlineVR => blend_modes
            .into_iter()
            .find(|b| *b == xr::EnvironmentBlendMode::OPAQUE)?,
        XrSessionMode::ImmersiveAR | XrSessionMode::InlineAR => blend_modes
            .iter()
            .cloned()
            .find(|b| *b == xr::EnvironmentBlendMode::ALPHA_BLEND)
            .or_else(|| {
                blend_modes
                    .into_iter()
                    .find(|b| *b == xr::EnvironmentBlendMode::ADDITIVE)
            })?,
    };

    Some((view_type, blend_mode))
}

#[derive(Default)]
pub struct OpenXrPlugin;

impl Plugin for OpenXrPlugin {
    fn build(&self, app: &mut App) {
        setup::setup_xrcontext_and_graphics(app);
        //  Populate this state before the runner so that plugins that run
        //  app.update() will have the expected resources (such as
        //  bevy_editor_pls).
        let runner_state = setup::setup_other_xr(app);
        app.insert_resource(runner_state);
    }
}


// Currently, only the session loop is implemented. If the session is destroyed or fails to
// create, the app will exit.
// todo: Implement the instance loop when the the lifecycle API is implemented.
fn runner(mut app: App) {
    let setup::XrRunnerState {
        tracking_context,
        view_type,
        mut app_exit_event_reader,
        interaction_context,
        mut frame_waiter,
        mut frame_stream,
        blend_mode,
        next_vsync_time,
        stage,
        vk_session,
        left_id,
        right_id,
        xr_context: ctx,
    }: setup::XrRunnerState = app.world.remove_resource().unwrap();
    let mut vibration_event_reader = ManualEventReader::default();

    let mut event_storage = xr::EventDataBuffer::new();

    let mut swapchain = None;
    let mut running = false;

    #[cfg(feature = "winit_loop")]
    let mut winit_state = crate::winit::State::new();
    #[cfg(feature = "winit_loop")]
    {
        crate::winit::init_window(&mut app);
    }

    let session = app.world.resource::<OpenXrSession>().clone();

    let mut frame_count = 0usize;
    'session_loop: loop {
        #[cfg(feature = "winit_loop")]
        {
            winit_state = crate::winit::run_event_loop(winit_state, &mut app);
        }

        frame_count += 1;
        while let Some(event) = ctx.instance.poll_event(&mut event_storage).unwrap() {
            match event {
                xr::Event::EventsLost(e) => {
                    bevy_log::error!("OpenXR: Lost {} events", e.lost_event_count());
                }
                xr::Event::InstanceLossPending(_) => {
                    bevy_log::info!("OpenXR: Shutting down for runtime request");
                    break 'session_loop;
                }
                xr::Event::SessionStateChanged(e) => {
                    bevy_log::debug!("entered state {:?}", e.state());

                    match e.state() {
                        xr::SessionState::UNKNOWN | xr::SessionState::IDLE => (),
                        xr::SessionState::READY => {
                            session.begin(view_type).unwrap();
                            running = true;
                        }
                        xr::SessionState::SYNCHRONIZED => {
                            app.world.insert_resource(XrVisibilityState::Hidden)
                        }
                        xr::SessionState::VISIBLE => app
                            .world
                            .insert_resource(XrVisibilityState::VisibleUnfocused),
                        xr::SessionState::FOCUSED => {
                            app.world.insert_resource(XrVisibilityState::VisibleFocused)
                        }
                        xr::SessionState::STOPPING => {
                            session.end().unwrap();
                            running = false;
                        }
                        xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                            println!("Exiting | Loss Pending");
                            break 'session_loop;
                        }
                        _ => unreachable!(),
                    }
                }
                xr::Event::ReferenceSpaceChangePending(e) => {
                    let reference_ref = &mut tracking_context.reference.write();

                    reference_ref.space_type = e.reference_space_type();
                    reference_ref.change_time = e.change_time();
                    reference_ref.previous_pose_offset =
                        openxr_pose_to_rigid_transform(e.pose_in_previous_space())
                }
                xr::Event::PerfSettingsEXT(e) => {
                    let sub_domain = match e.sub_domain() {
                        xr::PerfSettingsSubDomainEXT::COMPOSITING => "compositing",
                        xr::PerfSettingsSubDomainEXT::RENDERING => "rendering",
                        xr::PerfSettingsSubDomainEXT::THERMAL => "thermal",
                        _ => unreachable!(),
                    };
                    let domain = match e.domain() {
                        xr::PerfSettingsDomainEXT::CPU => "CPU",
                        xr::PerfSettingsDomainEXT::GPU => "GPU",
                        _ => unreachable!(),
                    };
                    let from = match e.from_level() {
                        xr::PerfSettingsNotificationLevelEXT::NORMAL => "normal",
                        xr::PerfSettingsNotificationLevelEXT::WARNING => "warning",
                        xr::PerfSettingsNotificationLevelEXT::IMPAIRED => "critical",
                        _ => unreachable!(),
                    };
                    let to = match e.to_level() {
                        xr::PerfSettingsNotificationLevelEXT::NORMAL => "normal",
                        xr::PerfSettingsNotificationLevelEXT::WARNING => "warning",
                        xr::PerfSettingsNotificationLevelEXT::IMPAIRED => "critical",
                        _ => unreachable!(),
                    };
                    bevy_log::warn!(
                        "OpenXR: The {} state of the {} went from {} to {}",
                        sub_domain,
                        domain,
                        from,
                        to
                    );

                    // todo: react to performance notifications
                }
                xr::Event::VisibilityMaskChangedKHR(_) => (), // todo: update visibility mask
                xr::Event::InteractionProfileChanged(_) => {
                    let left_hand = ctx
                        .instance
                        .path_to_string(
                            session
                                .current_interaction_profile(
                                    ctx.instance.string_to_path("/user/hand/left").unwrap(),
                                )
                                .unwrap(),
                        )
                        .ok();
                    let right_hand = ctx
                        .instance
                        .path_to_string(
                            session
                                .current_interaction_profile(
                                    ctx.instance.string_to_path("/user/hand/right").unwrap(),
                                )
                                .unwrap(),
                        )
                        .ok();

                    app.world.insert_resource(XrProfiles {
                        left_hand,
                        right_hand,
                    })
                }
                xr::Event::MainSessionVisibilityChangedEXTX(_) => (), // unused
                xr::Event::DisplayRefreshRateChangedFB(evt) => {
                    //  BUG: on oculus quest2 this will fire even when a requested refresh rate fails
                    bevy_log::info!(
                        "refresh rate changed: {} -> {}",
                        evt.from_display_refresh_rate(),
                        evt.to_display_refresh_rate()
                    );
                }
                _ => bevy_log::debug!("OpenXR: Unhandled event"),
            }
        }

        if !running {
            thread::sleep(Duration::from_millis(200));
            continue;
        }

        if frame_count % 1000 == 0 {
            utils::increase_refresh_rate(&ctx.instance, session.as_raw());
        }

        let frame_state = frame_waiter.wait().unwrap();
        session
            .sync_actions(&[(ActiveActionSet::new(&interaction_context.action_set.lock()))])
            .unwrap();

        frame_stream.begin().unwrap();

        if !frame_state.should_render {
            frame_stream
                .end(frame_state.predicted_display_time, blend_mode, &[])
                .unwrap();
            continue;
        }

        //  TODO: override bevy time with predicted frame time?
        *next_vsync_time.write() = frame_state.predicted_display_time;

        {
            let _world_cell = app.world.cell();
            handle_input(
                &interaction_context,
                &session,
                &mut _world_cell.get_resource_mut::<XrActionSet>().unwrap(),
            );
        }

        let (view_state_flags, views) = session
            .locate_views(view_type, frame_state.predicted_display_time, &stage)
            .unwrap();

        let view_cfgs = session
            .instance()
            .enumerate_view_configuration_views(ctx.system, view_type)
            .unwrap();

        // let resolutions: [vk::Extent2D; 2] = view_cfgs.iter().map();
        let resolutions: &Vec<vk::Extent2D> = &view_cfgs
            .iter()
            .map(|view_cfg| vk::Extent2D {
                width: view_cfg.recommended_image_rect_width,
                height: view_cfg.recommended_image_rect_height,
            })
            .collect();
        let device = ctx.wgpu_device.clone();
        let swapchains = swapchain
            .get_or_insert_with(|| EyeSwapchains::new(&vk_session, resolutions, device).unwrap());

        let left_tex = swapchains.left.acquire_texture_view().unwrap();
        let right_tex = swapchains.right.acquire_texture_view().unwrap();

        let mut manual_texture_views = app.world.get_resource_mut::<ManualTextureViews>().unwrap();
        manual_texture_views.insert(left_id, (left_tex.into(), resolutions[0].bevy()));
        manual_texture_views.insert(right_id, (right_tex.into(), resolutions[1].bevy()));

        app.world.insert_resource(XrViews(views.clone()));

        app.update();

        swapchains.left.release().unwrap();
        swapchains.right.release().unwrap();

        if view_state_flags
            .contains(ViewStateFlags::POSITION_VALID | ViewStateFlags::ORIENTATION_VALID)
        {
            frame_stream
                .end(
                    frame_state.predicted_display_time,
                    blend_mode,
                    &[
                        &xr::CompositionLayerProjection::new().space(&stage).views(&[
                            xr::CompositionLayerProjectionView::new()
                                .pose(views[0].pose)
                                .fov(views[0].fov)
                                .sub_image(
                                    xr::SwapchainSubImage::new()
                                        .swapchain(&swapchains.left.handle)
                                        .image_rect(resolutions[0].xr()),
                                ),
                            xr::CompositionLayerProjectionView::new()
                                .pose(views[1].pose)
                                .fov(views[1].fov)
                                .sub_image(
                                    xr::SwapchainSubImage::new()
                                        .swapchain(&swapchains.right.handle)
                                        .image_rect(resolutions[1].xr()),
                                ),
                        ]),
                    ],
                )
                .unwrap()
        } else {
            frame_stream
                .end(frame_state.predicted_display_time, blend_mode, &[])
                .unwrap()
        }

        handle_output(
            &interaction_context,
            &session,
            &mut vibration_event_reader,
            &mut app
                .world
                .get_resource_mut::<Events<XrVibrationEvent>>()
                .unwrap(),
        );

        if app_exit_event_reader
            .iter(&app.world.get_resource_mut::<Events<AppExit>>().unwrap())
            .next_back()
            .is_some()
        {
            println!("app exit event");
            session.request_exit().unwrap();
        }
    }
    println!("runner loop done");
}
