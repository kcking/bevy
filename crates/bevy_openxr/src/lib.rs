mod camera;
mod conversion;
use bevy_core_pipeline::{CameraLeftEye, CameraRightEye};
use bevy_hierarchy::BuildWorldChildren;
use camera::{XRCameraBundle, XRProjection};
use conversion::*;
mod interaction;
mod presentation;
mod swapchain;
use swapchain::*;

use ash::vk;
use ash::vk::Handle;
use bevy_math::{Quat, UVec2, Vec3};
use bevy_render::{
    camera::{
        camera_system, ActiveCamera, Camera, CameraProjection, ManualTextureViews,
        PerspectiveCameraBundle, PerspectiveProjection, RenderTarget,
    },
    prelude::{Color, Msaa},
    primitives::Frustum,
    view::{update_frusta, VisibilitySystems},
};
use bevy_transform::{
    components::{GlobalTransform, Transform},
    TransformSystem,
};
use bevy_utils::Uuid;
pub use interaction::*;

use bevy_app::{App, AppExit, CoreStage, Plugin};
use bevy_ecs::{
    entity::Entity,
    event::{Events, ManualEventReader},
    prelude::{Bundle, Component, Without, World},
    schedule::{ParallelSystemDescriptorCoercion, Schedule},
    system::{Commands, IntoSystem, Query, Res, System},
    world::EntityMut,
};
use bevy_xr::{
    presentation::{XrEnvironmentBlendMode, XrGraphicsContext, XrInteractionMode},
    XrProfiles, XrSessionMode, XrSystem, XrTrackingSource, XrVibrationEvent, XrVisibilityState,
};
use openxr::{self as xr, sys};
use parking_lot::RwLock;
use presentation::GraphicsContextHandles;
use serde::{Deserialize, Serialize};
use xr::{Quaternionf, Vector3f, View};

use std::{error::Error, ops::Deref, sync::Arc, thread, time::Duration};
use wgpu::{TextureUsages, TextureViewDescriptor};
use wgpu_hal::TextureUses;

// The form-factor is selected at plugin-creation-time and cannot be changed anymore for the entire
// lifetime of the app. This will restrict which XrSessionMode can be selected.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum OpenXrFormFactor {
    HeadMountedDisplay,
    Handheld,
}

enum SessionBackend {
    Vulkan(xr::Session<xr::Vulkan>),
    #[cfg(windows)]
    D3D11(xr::Session<xr::D3D11>),
}

enum FrameStream {
    Vulkan(xr::FrameStream<xr::Vulkan>),
    #[cfg(windows)]
    D3D11(xr::FrameStream<xr::D3D11>),
}

#[derive(Clone)]
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
    #[cfg(any(target_os = "android", target_os = "macos"))]
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
    exts.khr_vulkan_enable2 = available.khr_vulkan_enable2;
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
        // todo: set APPLICATION_MAIN and RENDER_MAIN threads
    }
    #[cfg(windows)]
    {
        exts.khr_d3d11_enable = available.khr_d3d11_enable;
    }

    exts
}

pub struct OpenXrContext {
    instance: xr::Instance,
    form_factor: xr::FormFactor,
    system: xr::SystemId,
    // Note: the lifecycle of graphics handles is managed by wgpu objects
    graphics_handles: GraphicsContextHandles,
    wgpu_device: Arc<wgpu::Device>,
    graphics_context: Option<XrGraphicsContext>,
}

impl OpenXrContext {
    fn new(form_factor: OpenXrFormFactor) -> Result<Self, OpenXrError> {
        #[cfg(any(target_os = "android", target_os = "macos"))]
        let entry = xr::Entry::load().map_err(OpenXrError::Loader)?;
        #[cfg(not(any(target_os = "android", target_os = "macos")))]
        let entry = xr::Entry::linked();

        #[cfg(target_os = "android")]
        entry.initialize_android_loader();

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
        if !app.world.contains_resource::<OpenXrContext>() {
            let context =
                OpenXrContext::new(OpenXrFormFactor::HeadMountedDisplay).unwrap_or_else(|_| {
                    match OpenXrContext::new(OpenXrFormFactor::Handheld) {
                        Ok(context) => context,
                        // In case OpenXR is suported, there should be always at least one supported
                        // form factor. If "Handheld" is unsupported, "HeadMountedDisplay" is
                        // supported (but in this case unavailable).
                        Err(
                            OpenXrError::UnsupportedFormFactor | OpenXrError::UnavailableFormFactor,
                        ) => panic!(
                            "OpenXR: No available form factors. Consider manually handling {}",
                            "the creation of the OpenXrContext resource."
                        ),
                        Err(OpenXrError::InstanceCreation(sys::Result::ERROR_RUNTIME_FAILURE)) => {
                            panic!(
                                "OpenXR: Failed to create OpenXrContext: {:?}\n{} {}",
                                sys::Result::ERROR_RUNTIME_FAILURE,
                                "Is your headset connected? Also, consider manually handling",
                                "the creation of the OpenXrContext resource."
                            )
                        }
                        Err(e) => panic!(
                            "OpenXR: Failed to create OpenXrContext: {:?}\n{} {}",
                            e,
                            "Consider manually handling",
                            "the creation of the OpenXrContext resource."
                        ),
                    }
                });
            app.world.insert_resource(context);
        }

        let mut context = app.world.get_resource_mut::<OpenXrContext>().unwrap();
        let graphics_context = context.graphics_context.take().unwrap();
        println!("got graphics context");

        let dev = bevy_render::renderer::RenderDevice::from(graphics_context.device.clone());
        let queue = bevy_render::renderer::RenderQueue::from(graphics_context.queue.clone());

        //override default render stuff
        app.insert_resource(dev).insert_resource(queue);

        app.insert_resource::<XrGraphicsContext>(graphics_context)
            .set_runner(runner);

        //  XXX: multi-sampling with RenderTarget::TextureView doesn't work currently
        app.insert_resource(Msaa { samples: 1 });

        app.register_type::<XRProjection>();
        app.add_system_to_stage(
            CoreStage::PostUpdate,
            update_frusta::<XRProjection>
                .after(TransformSystem::TransformPropagate)
                //  ensures we execute at the right time without adding more labels
                .before(VisibilitySystems::UpdatePerspectiveFrusta),
        );
        app.add_system_to_stage(CoreStage::PostUpdate, camera_system::<XRProjection>);
    }
}

// Currently, only the session loop is implemented. If the session is destroyed or fails to
// create, the app will exit.
// todo: Implement the instance loop when the the lifecycle API is implemented.
fn runner(mut app: App) {
    let ctx = app.world.remove_resource::<OpenXrContext>().unwrap();

    app.world.insert_resource(ctx.instance.clone());

    let mut app_exit_event_reader = ManualEventReader::default();

    let interaction_mode = if ctx.form_factor == xr::FormFactor::HEAD_MOUNTED_DISPLAY {
        XrInteractionMode::WorldSpace
    } else {
        XrInteractionMode::ScreenSpace
    };
    app.world.insert_resource(interaction_mode);

    // Find the available session modes
    let available_session_modes = [
        XrSessionMode::ImmersiveVR,
        XrSessionMode::ImmersiveAR,
        XrSessionMode::InlineVR,
        XrSessionMode::InlineAR,
    ]
    .iter()
    .filter_map(|mode| get_system_info(&ctx.instance, ctx.system, *mode).map(|_| *mode))
    .collect();

    app.world
        .insert_resource(XrSystem::new(available_session_modes));
    println!("inserted XrSystem");

    if app_exit_event_reader
        .iter(&app.world.get_resource_mut::<Events<AppExit>>().unwrap())
        .next_back()
        .is_some()
    {
        return;
    }

    let xr_system = app.world.get_resource::<XrSystem>().unwrap();

    let mode = xr_system.selected_session_mode();
    let bindings = xr_system.action_set();

    let interaction_context = InteractionContext::new(&ctx.instance, bindings);

    // Remove XrSystem. The user cannot make any more changes to the session mode.
    // todo: when the lifecycle API is implemented, allow the user to change the session mode at any
    // moment.
    // app.world.remove_resource::<XrSystem>();

    let (view_type, blend_mode) = get_system_info(&ctx.instance, ctx.system, mode).unwrap();

    let environment_blend_mode = match blend_mode {
        xr::EnvironmentBlendMode::OPAQUE => XrEnvironmentBlendMode::Opaque,
        xr::EnvironmentBlendMode::ALPHA_BLEND => XrEnvironmentBlendMode::AlphaBlend,
        xr::EnvironmentBlendMode::ADDITIVE => XrEnvironmentBlendMode::Additive,
        _ => unreachable!(),
    };
    app.world.insert_resource(environment_blend_mode);

    let (vk_session, session, _graphics_session, mut frame_waiter, mut frame_stream) =
        match ctx.graphics_handles {
            GraphicsContextHandles::Vulkan {
                instance,
                physical_device,
                device,
                queue_family_index,
                queue_index,
            } => {
                let (session, frame_waiter, frame_stream) = unsafe {
                    ctx.instance
                        .create_session(
                            ctx.system,
                            &xr::vulkan::SessionCreateInfo {
                                instance: instance.handle().as_raw() as *const _,
                                physical_device: physical_device.as_raw() as *const _,
                                device: device.handle().as_raw() as *const _,
                                queue_family_index,
                                queue_index,
                            },
                        )
                        .unwrap()
                };
                (
                    session.clone(),
                    session.clone().into_any_graphics(),
                    SessionBackend::Vulkan(session),
                    frame_waiter,
                    FrameStream::Vulkan(frame_stream),
                )
            }
        };

    let session = OpenXrSession {
        inner: Some(session),
        _wgpu_device: ctx.wgpu_device.clone(),
    };

    // The user can have a limited access to the OpenXR session using OpenXrSession, which is
    // clonable but safe because of the _wgpu_device internal handle.
    app.world.insert_resource(session.clone());

    session
        .attach_action_sets(&[&interaction_context.action_set.lock()])
        .unwrap();

    let tracking_context = Arc::new(OpenXrTrackingContext::new(
        &ctx.instance,
        ctx.system,
        &interaction_context,
        session.clone(),
    ));

    let next_vsync_time = Arc::new(RwLock::new(xr::Time::from_nanos(0)));

    let tracking_source = TrackingSource {
        view_type,
        action_set: interaction_context.action_set.clone(),
        session: session.clone(),
        context: tracking_context.clone(),
        next_vsync_time: next_vsync_time.clone(),
    };

    app.world.insert_resource(tracking_context.clone());
    app.world
        .insert_resource(XrTrackingSource::new(Box::new(tracking_source)));

    // todo: use these views limits and recommendations
    let _views = ctx
        .instance
        .enumerate_view_configuration_views(ctx.system, view_type)
        .unwrap();

    let stage = session
        .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)
        .unwrap();

    let mut vibration_event_reader = ManualEventReader::default();

    let mut event_storage = xr::EventDataBuffer::new();

    let mut swapchain = None;
    let mut running = false;

    let left_id = Uuid::new_v4();
    let right_id = Uuid::new_v4();
    XrCameras::spawn(app.world.spawn(), left_id, right_id);
    app.add_system_to_stage(CoreStage::PreUpdate, update_xrcamera_view);

    let clear_color_default = Color::rgb(0.4, 0.4, 0.4);
    let mut clear_color = app
        .world
        .get_resource_mut::<bevy_core_pipeline::ClearColor>()
        .unwrap();
    clear_color.insert(
        bevy_render::camera::RenderTarget::TextureView(right_id),
        clear_color_default,
    );
    clear_color.insert(
        bevy_render::camera::RenderTarget::TextureView(left_id),
        clear_color_default,
    );

    'session_loop: loop {
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
                xr::Event::DisplayRefreshRateChangedFB(_) => (),      // shouldn't be needed
                _ => bevy_log::debug!("OpenXR: Unhandled event"),
            }
        }

        if !running {
            thread::sleep(Duration::from_millis(200));
            continue;
        }

        let frame_state = frame_waiter.wait().unwrap();

        match &mut frame_stream {
            FrameStream::Vulkan(frame_stream) => frame_stream.begin().unwrap(),
            #[cfg(windows)]
            FrameStream::D3D11(frame_stream) => frame_stream.begin().unwrap(),
        }

        if !frame_state.should_render {
            match &mut frame_stream {
                FrameStream::Vulkan(frame_stream) => frame_stream
                    .end(frame_state.predicted_display_time, blend_mode, &[])
                    .unwrap(),
                #[cfg(windows)]
                FrameStream::D3D11(_) => todo!(),
            }
            continue;
        }

        //  TODO: override bevy time with predicted frame time?
        *next_vsync_time.write() = frame_state.predicted_display_time;

        {
            let world_cell = app.world.cell();
            // handle_input(
            //     &interaction_context,
            //     &session,
            //     &mut world_cell.get_resource_mut::<XrActionSet>().unwrap(),
            // );
        }

        let (_, views) = session
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

        app.world.insert_resource(views.clone());

        app.update();

        swapchains.left.release().unwrap();
        swapchains.right.release().unwrap();

        match &mut frame_stream {
            FrameStream::Vulkan(frame_stream) => frame_stream
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
                .unwrap(),
            #[cfg(windows)]
            FrameStream::D3D11(frame_stream) => frame_stream
                .end(frame_state.predicted_display_time, blend_mode, todo!())
                .unwrap(),
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
            session.request_exit().unwrap();
        }
    }
}

#[derive(Component)]
pub struct XrCameras {}

#[derive(Component, Debug)]
pub enum Eye {
    Left,
    Right,
}

impl XrCameras {
    pub fn spawn(mut e: EntityMut, left_id: Uuid, right_id: Uuid) {
        let (mut left_out, mut right_out) = (None, None);
        e.with_children(|parent| {
            let left = parent
                .spawn_bundle(XRCameraBundle {
                    camera: Camera {
                        target: RenderTarget::TextureView(left_id),
                        ..Default::default()
                    },
                    marker: CameraLeftEye,
                    ..Default::default()
                })
                .insert(Eye::Left)
                .id();
            let right = parent
                .spawn_bundle(XRCameraBundle {
                    camera: Camera {
                        target: RenderTarget::TextureView(right_id),
                        ..Default::default()
                    },
                    marker: CameraRightEye,
                    ..Default::default()
                })
                .insert(Eye::Right)
                .id();

            let _ = left_out.insert(left);
            let _ = right_out.insert(right);
        })
        .insert(Transform::default())
        .insert(GlobalTransform::default())
        .insert(Self {});

        let mut active_left = ActiveCamera::<CameraLeftEye>::default();
        active_left.set(left_out.unwrap());
        unsafe { e.world_mut() }.insert_resource(active_left);
        let mut active_right = ActiveCamera::<CameraLeftEye>::default();
        active_right.set(right_out.unwrap());
        unsafe { e.world_mut() }.insert_resource(active_right);
    }
}

trait Vec3Conv {
    fn to_vec3(&self) -> Vec3;
}

impl Vec3Conv for Vector3f {
    fn to_vec3(&self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
}

trait QuatConv {
    fn to_quat(&self) -> Quat;
}

impl QuatConv for Quaternionf {
    fn to_quat(&self) -> Quat {
        Quat::from_xyzw(self.x, self.y, self.z, self.w)
    }
}

pub fn update_xrcamera_view(
    mut cam: Query<(&mut XRProjection, &mut Transform, &Eye)>,
    mut xr_cam: Query<(&mut Transform, &XrCameras), Without<Eye>>,
    views: Res<Vec<View>>,
) {
    let midpoint = (views.get(0).unwrap().pose.position.to_vec3()
        + views.get(1).unwrap().pose.position.to_vec3())
        / 2.;
    xr_cam.single_mut().0.translation = midpoint;

    let left_rot = views.get(0).unwrap().pose.orientation.to_quat();
    let right_rot = views.get(1).unwrap().pose.orientation.to_quat();
    let mid_rot = if left_rot.dot(right_rot) >= 0. {
        left_rot.slerp(right_rot, 0.5)
    } else {
        right_rot.slerp(left_rot, 0.5)
    };
    let mid_rot_inverse = mid_rot.inverse();
    xr_cam.single_mut().0.rotation = mid_rot;

    for (mut projection, mut transform, eye) in cam.iter_mut() {
        let view_idx = match eye {
            Eye::Left => 0,
            Eye::Right => 1,
        };
        let view = views.get(view_idx).unwrap();

        projection.fov = view.fov;

        transform.rotation = mid_rot_inverse * view.pose.orientation.to_quat();
        let pos = view.pose.position;
        transform.translation = pos.to_vec3() - midpoint;
    }
}
