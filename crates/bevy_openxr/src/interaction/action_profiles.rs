use crate::*;

pub fn setup_interaction(system: &mut XrSystem) {
    if cfg!(target_os = "android") {
        let oculus_profile = XrProfileDescriptor {
            profile: OCULUS_TOUCH_PROFILE.into(),
            bindings: vec![
                (
                    XrActionDescriptor {
                        name: "left_trigger".into(),
                        action_type: XrActionType::Button {
                            touch: true,
                            click: false,
                            value: true,
                        },
                    },
                    "/user/hand/left/input/trigger".into(),
                ),
                (
                    XrActionDescriptor {
                        name: "left_primary".into(),
                        action_type: XrActionType::Button {
                            touch: true,
                            click: true,
                            value: false,
                        },
                    },
                    "/user/hand/left/input/x".into(),
                ),
                (
                    XrActionDescriptor {
                        name: "right_trigger".into(),
                        action_type: XrActionType::Button {
                            touch: true,
                            click: false,
                            value: true,
                        },
                    },
                    "/user/hand/right/input/trigger".into(),
                ),
                (
                    XrActionDescriptor {
                        name: "right_primary".into(),
                        action_type: XrActionType::Button {
                            touch: true,
                            click: true,
                            value: false,
                        },
                    },
                    "/user/hand/right/input/a".into(),
                ),
            ],
            tracked: true,
            has_haptics: true,
        };
        system.set_action_set(vec![oculus_profile]);
    } else {
        //  TODO: use runtime settings or build-time features to pick active Profile
        let action_set = XrProfileDescriptor {
            profile: VALVE_INDEX_PROFILE.into(),
            bindings: vec![
                (
                    XrActionDescriptor {
                        name: "left_trigger".into(),
                        action_type: XrActionType::Button {
                            touch: true,
                            click: true,
                            value: true,
                        },
                    },
                    "/user/hand/left/input/trigger".into(),
                ),
                (
                    XrActionDescriptor {
                        name: "left_primary".into(),
                        action_type: XrActionType::Button {
                            touch: true,
                            click: true,
                            value: false,
                        },
                    },
                    "/user/hand/left/input/a".into(),
                ),
                (
                    XrActionDescriptor {
                        name: "left_secondary".into(),
                        action_type: XrActionType::Button {
                            touch: true,
                            click: true,
                            value: false,
                        },
                    },
                    "/user/hand/left/input/b".into(),
                ),
                (
                    XrActionDescriptor {
                        name: "right_trigger".into(),
                        action_type: XrActionType::Button {
                            touch: true,
                            click: true,
                            value: true,
                        },
                    },
                    "/user/hand/right/input/trigger".into(),
                ),
                (
                    XrActionDescriptor {
                        name: "right_primary".into(),
                        action_type: XrActionType::Button {
                            touch: true,
                            click: true,
                            value: false,
                        },
                    },
                    "/user/hand/right/input/a".into(),
                ),
                (
                    XrActionDescriptor {
                        name: "right_secondary".into(),
                        action_type: XrActionType::Button {
                            touch: true,
                            click: true,
                            value: false,
                        },
                    },
                    "/user/hand/right/input/a".into(),
                ),
            ],
            tracked: true,
            has_haptics: true,
        };
        system.set_action_set(vec![action_set]);
    }
}
