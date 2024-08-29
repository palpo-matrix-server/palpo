// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "tsvector", schema = "pg_catalog"))]
    pub struct Tsvector;
}

diesel::table! {
    appservice_registrations (id) {
        id -> Text,
        url -> Nullable<Text>,
        as_token -> Text,
        hs_token -> Text,
        sender_localpart -> Text,
        namespaces -> Json,
        rate_limited -> Nullable<Bool>,
        protocols -> Nullable<Json>,
    }
}

diesel::table! {
    device_inboxes (id) {
        id -> Int8,
        user_id -> Text,
        device_id -> Text,
        json_data -> Json,
        occur_sn -> Int8,
        created_at -> Int8,
    }
}

diesel::table! {
    device_streams (id) {
        id -> Int8,
        user_id -> Text,
        device_id -> Text,
    }
}

diesel::table! {
    e2e_cross_signing_keys (id) {
        id -> Int8,
        user_id -> Text,
        key_type -> Text,
        key_data -> Json,
    }
}

diesel::table! {
    e2e_cross_signing_sigs (id) {
        id -> Int8,
        origin_user_id -> Text,
        origin_key_id -> Text,
        target_user_id -> Text,
        target_device_id -> Text,
        signature -> Text,
    }
}

diesel::table! {
    e2e_device_keys (id) {
        id -> Int8,
        user_id -> Text,
        device_id -> Text,
        stream_id -> Int8,
        display_name -> Nullable<Text>,
        key_data -> Json,
        created_at -> Int8,
    }
}

diesel::table! {
    e2e_fallback_keys (id) {
        id -> Int8,
        user_id -> Text,
        device_id -> Text,
        algorithm -> Text,
        key_id -> Text,
        key_data -> Json,
        used_at -> Nullable<Int8>,
        created_at -> Int8,
    }
}

diesel::table! {
    e2e_key_changes (id) {
        id -> Int8,
        user_id -> Text,
        room_id -> Nullable<Text>,
        occur_sn -> Int8,
        changed_at -> Int8,
    }
}

diesel::table! {
    e2e_one_time_keys (id) {
        id -> Int8,
        user_id -> Text,
        device_id -> Text,
        algorithm -> Text,
        key_id -> Text,
        key_data -> Json,
        created_at -> Int8,
    }
}

diesel::table! {
    e2e_room_keys (id) {
        id -> Int8,
        user_id -> Text,
        room_id -> Text,
        session_id -> Text,
        version -> Int8,
        first_message_index -> Nullable<Int8>,
        forwarded_count -> Nullable<Int8>,
        is_verified -> Bool,
        session_data -> Json,
        created_at -> Int8,
    }
}

diesel::table! {
    e2e_room_keys_versions (id) {
        id -> Int8,
        user_id -> Text,
        version -> Int8,
        algorithm -> Json,
        auth_data -> Json,
        is_trashed -> Bool,
        etag -> Int8,
        created_at -> Int8,
    }
}

diesel::table! {
    event_auth_chains (event_id) {
        event_id -> Text,
        chain_id -> Int8,
        sequence_number -> Int8,
    }
}

diesel::table! {
    event_backward_extremities (id) {
        id -> Int8,
        event_id -> Text,
        room_id -> Text,
    }
}

diesel::table! {
    event_datas (event_id) {
        event_id -> Text,
        event_sn -> Int8,
        room_id -> Text,
        internal_metadata -> Nullable<Json>,
        format_version -> Nullable<Int8>,
        json_data -> Json,
    }
}

diesel::table! {
    event_edges (event_id) {
        event_id -> Text,
        prev_event_id -> Text,
        room_id -> Nullable<Text>,
        is_state -> Bool,
    }
}

diesel::table! {
    event_forward_extremities (id) {
        id -> Int8,
        event_id -> Text,
        room_id -> Text,
    }
}

diesel::table! {
    event_push_summaries (id) {
        id -> Int8,
        user_id -> Text,
        room_id -> Text,
        notification_count -> Int8,
        highlight_count -> Int8,
        unread_count -> Int8,
        stream_ordering -> Int8,
        thread_id -> Nullable<Text>,
    }
}

diesel::table! {
    event_receipts (id) {
        id -> Int8,
        room_id -> Text,
        receipt_type -> Text,
        user_id -> Text,
        event_id -> Text,
        event_sn -> Int8,
        receipt_at -> Int8,
    }
}

diesel::table! {
    event_relations (id) {
        id -> Int8,
        room_id -> Text,
        event_id -> Text,
        event_sn -> Int8,
        child_id -> Text,
        child_sn -> Int8,
        child_event_type -> Text,
        rel_type -> Nullable<Text>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::Tsvector;

    event_searches (id) {
        id -> Int8,
        event_id -> Text,
        room_id -> Nullable<Text>,
        sender -> Nullable<Text>,
        key -> Text,
        vector -> Nullable<Tsvector>,
        origin_server_ts -> Nullable<Int8>,
        stream_ordering -> Nullable<Int8>,
    }
}

diesel::table! {
    event_txn_ids (event_id) {
        event_id -> Text,
        room_id -> Text,
        user_id -> Text,
        device_id -> Nullable<Text>,
        txn_id -> Text,
        created_at -> Int8,
    }
}

diesel::table! {
    events (id) {
        id -> Text,
        sn -> Int8,
        event_type -> Text,
        room_id -> Text,
        unrecognized_keys -> Nullable<Text>,
        depth -> Int8,
        origin_server_ts -> Nullable<Int8>,
        received_at -> Nullable<Int8>,
        sender -> Nullable<Text>,
        contains_url -> Bool,
        worker_id -> Nullable<Text>,
        state_key -> Nullable<Text>,
        processed -> Bool,
        outlier -> Bool,
        soft_failed -> Bool,
        rejection_reason -> Nullable<Text>,
    }
}

diesel::table! {
    lazy_load_deliveries (id) {
        id -> Int8,
        user_id -> Text,
        device_id -> Text,
        room_id -> Text,
        confirmed_user_id -> Nullable<Text>,
    }
}

diesel::table! {
    media_metadatas (id) {
        id -> Int8,
        media_id -> Text,
        origin_server -> Text,
        content_type -> Text,
        upload_name -> Text,
        file_extension -> Nullable<Text>,
        file_size -> Int8,
        hash -> Text,
        created_by -> Nullable<Text>,
        created_at -> Int8,
    }
}

diesel::table! {
    media_thumbnails (id) {
        id -> Int8,
        media_id -> Text,
        origin_server -> Text,
        content_type -> Text,
        file_size -> Int8,
        width -> Int4,
        height -> Int4,
        resize_method -> Text,
        created_at -> Int8,
    }
}

diesel::table! {
    room_aliases (alias) {
        alias -> Text,
        room_id -> Text,
        created_by -> Text,
        created_at -> Int8,
    }
}

diesel::table! {
    room_servers (id) {
        id -> Int8,
        room_id -> Text,
        server_id -> Text,
    }
}

diesel::table! {
    room_state_deltas (frame_id) {
        frame_id -> Int8,
        room_id -> Text,
        parent_id -> Nullable<Int8>,
        append_data -> Bytea,
        remove_data -> Bytea,
    }
}

diesel::table! {
    room_state_fields (id) {
        id -> Int8,
        event_type -> Text,
        state_key -> Text,
    }
}

diesel::table! {
    room_state_frames (id) {
        id -> Int8,
        room_id -> Text,
        hash_data -> Bytea,
    }
}

diesel::table! {
    room_state_points (id) {
        id -> Int8,
        room_id -> Text,
        event_id -> Text,
        event_sn -> Int8,
        frame_id -> Nullable<Int8>,
    }
}

diesel::table! {
    room_tags (id) {
        id -> Int8,
        user_id -> Text,
        room_id -> Text,
        tag -> Text,
        content -> Json,
        created_by -> Text,
        created_at -> Int8,
    }
}

diesel::table! {
    room_threads (id) {
        id -> Text,
        room_id -> Text,
        latest_event_id -> Text,
        topological_ordering -> Int8,
        stream_ordering -> Int8,
    }
}

diesel::table! {
    room_users (id) {
        id -> Int8,
        event_id -> Text,
        event_sn -> Int8,
        room_id -> Text,
        user_id -> Text,
        sender_id -> Text,
        membership -> Text,
        forgotten -> Bool,
        display_name -> Nullable<Text>,
        avatar_url -> Nullable<Text>,
        state_data -> Nullable<Json>,
        created_at -> Int8,
    }
}

diesel::table! {
    rooms (id) {
        id -> Text,
        version -> Text,
        is_public -> Bool,
        has_auth_chain_index -> Bool,
        disabled -> Bool,
        state_frame_id -> Nullable<Int8>,
        created_by -> Text,
        created_at -> Int8,
    }
}

diesel::table! {
    server_signing_keys (server_id) {
        server_id -> Text,
        key_data -> Json,
        updated_at -> Int8,
        created_at -> Int8,
    }
}

diesel::table! {
    stats_monthly_active_users (id) {
        id -> Int8,
        user_id -> Text,
        created_at -> Int8,
    }
}

diesel::table! {
    stats_room_currents (room_id) {
        room_id -> Text,
        state_events -> Int8,
        joined_members -> Int8,
        invited_members -> Int8,
        left_members -> Int8,
        banned_members -> Int8,
        knocked_members -> Int8,
        local_users_in_room -> Int8,
        completed_delta_stream_id -> Int8,
    }
}

diesel::table! {
    stats_user_daily_visits (id) {
        id -> Int8,
        user_id -> Text,
        device_id -> Text,
        user_agent -> Nullable<Text>,
        created_at -> Int8,
    }
}

diesel::table! {
    thread_users (id) {
        id -> Int8,
        thread_id -> Text,
        user_id -> Text,
    }
}

diesel::table! {
    threads (id) {
        id -> Text,
        room_id -> Text,
        latest_event_id -> Text,
        topological_ordering -> Int8,
        stream_ordering -> Int8,
    }
}

diesel::table! {
    threepid_guests (id) {
        id -> Int8,
        medium -> Nullable<Text>,
        address -> Nullable<Text>,
        access_token -> Nullable<Text>,
        first_inviter -> Nullable<Text>,
        created_at -> Int8,
    }
}

diesel::table! {
    threepid_id_servers (id) {
        id -> Int8,
        user_id -> Text,
        medium -> Text,
        address -> Text,
        id_server -> Text,
    }
}

diesel::table! {
    threepid_validation_sessions (id) {
        id -> Int8,
        session_id -> Text,
        medium -> Text,
        address -> Text,
        client_secret -> Text,
        last_send_attempt -> Int8,
        validated_at -> Nullable<Int8>,
        created_at -> Int8,
    }
}

diesel::table! {
    threepid_validation_tokens (id) {
        id -> Int8,
        token -> Text,
        session_id -> Text,
        next_link -> Nullable<Text>,
        expired_at -> Int8,
        created_at -> Int8,
    }
}

diesel::table! {
    user_access_tokens (id) {
        id -> Int8,
        user_id -> Text,
        device_id -> Text,
        token -> Text,
        puppets_user_id -> Nullable<Text>,
        last_validated -> Nullable<Int8>,
        refresh_token_id -> Nullable<Int8>,
        is_used -> Bool,
        expired_at -> Nullable<Int8>,
        created_at -> Int8,
    }
}

diesel::table! {
    user_datas (id) {
        id -> Int8,
        user_id -> Text,
        room_id -> Nullable<Text>,
        data_type -> Text,
        json_data -> Json,
        occur_sn -> Int8,
        created_at -> Int8,
    }
}

diesel::table! {
    user_dehydrated_devices (id) {
        id -> Int8,
        user_id -> Text,
        device_id -> Text,
        device_data -> Json,
    }
}

diesel::table! {
    user_devices (id) {
        id -> Int8,
        user_id -> Text,
        device_id -> Text,
        display_name -> Nullable<Text>,
        user_agent -> Nullable<Text>,
        is_hidden -> Bool,
        last_seen_ip -> Nullable<Text>,
        last_seen_at -> Nullable<Int8>,
        created_at -> Int8,
    }
}

diesel::table! {
    user_filters (id) {
        id -> Int8,
        user_id -> Text,
        filter -> Json,
        created_at -> Int8,
    }
}

diesel::table! {
    user_ignores (id) {
        id -> Int8,
        user_id -> Text,
        ignored_id -> Text,
        created_at -> Int8,
    }
}

diesel::table! {
    user_openid_tokens (id) {
        id -> Int8,
        user_id -> Text,
        token -> Text,
        expires_at -> Int8,
    }
}

diesel::table! {
    user_passwords (id) {
        id -> Int8,
        user_id -> Text,
        hash -> Text,
        created_at -> Int8,
    }
}

diesel::table! {
    user_presences (id) {
        id -> Int8,
        user_id -> Text,
        room_id -> Nullable<Text>,
        stream_id -> Nullable<Int8>,
        state -> Nullable<Text>,
        status_msg -> Nullable<Text>,
        last_active_at -> Nullable<Int8>,
        last_federation_update_at -> Nullable<Int8>,
        last_user_sync_at -> Nullable<Int8>,
        currently_active -> Nullable<Bool>,
    }
}

diesel::table! {
    user_profiles (id) {
        id -> Int8,
        user_id -> Text,
        room_id -> Nullable<Text>,
        display_name -> Nullable<Text>,
        avatar_url -> Nullable<Text>,
        blurhash -> Nullable<Text>,
    }
}

diesel::table! {
    user_pushers (id) {
        id -> Int8,
        user_id -> Text,
        kind -> Text,
        app_id -> Text,
        app_display_name -> Text,
        device_display_name -> Text,
        access_token_id -> Nullable<Int8>,
        profile_tag -> Nullable<Text>,
        pushkey -> Text,
        lang -> Text,
        data -> Json,
        enabled -> Bool,
        last_stream_ordering -> Nullable<Int8>,
        last_success -> Nullable<Int8>,
        failing_since -> Nullable<Int8>,
        crated_at -> Int8,
    }
}

diesel::table! {
    user_refresh_tokens (id) {
        id -> Int8,
        user_id -> Text,
        device_id -> Text,
        token -> Text,
        next_token_id -> Nullable<Int8>,
        expired_at -> Nullable<Int8>,
        ultimate_session_expired_at -> Nullable<Int8>,
        created_at -> Int8,
    }
}

diesel::table! {
    user_registration_tokens (id) {
        id -> Int8,
        token -> Text,
        uses_allowed -> Nullable<Int8>,
        pending -> Int8,
        completed -> Int8,
        expired_at -> Nullable<Int8>,
        created_at -> Int8,
    }
}

diesel::table! {
    user_sessions (id) {
        id -> Int8,
        user_id -> Text,
        session_id -> Text,
        session_type -> Text,
        value -> Json,
        expired_at -> Int8,
        created_at -> Int8,
    }
}

diesel::table! {
    user_threepids (id) {
        id -> Int8,
        user_id -> Text,
        medium -> Text,
        address -> Text,
        validated_at -> Int8,
        added_at -> Int8,
    }
}

diesel::table! {
    user_uiaa_datas (id) {
        id -> Int8,
        user_id -> Text,
        device_id -> Text,
        session -> Text,
        uiaa_info -> Json,
    }
}

diesel::table! {
    users (id) {
        id -> Text,
        user_type -> Nullable<Text>,
        is_admin -> Bool,
        is_guest -> Bool,
        appservice_id -> Nullable<Text>,
        shadow_banned -> Bool,
        consent_at -> Nullable<Int8>,
        consent_version -> Nullable<Text>,
        consent_server_notice_sent -> Nullable<Text>,
        approved_at -> Nullable<Int8>,
        approved_by -> Nullable<Text>,
        deactivated_at -> Nullable<Int8>,
        deactivated_by -> Nullable<Text>,
        locked_at -> Nullable<Int8>,
        locked_by -> Nullable<Text>,
        created_at -> Int8,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    appservice_registrations,
    device_inboxes,
    device_streams,
    e2e_cross_signing_keys,
    e2e_cross_signing_sigs,
    e2e_device_keys,
    e2e_fallback_keys,
    e2e_key_changes,
    e2e_one_time_keys,
    e2e_room_keys,
    e2e_room_keys_versions,
    event_auth_chains,
    event_backward_extremities,
    event_datas,
    event_edges,
    event_forward_extremities,
    event_push_summaries,
    event_receipts,
    event_relations,
    event_searches,
    event_txn_ids,
    events,
    lazy_load_deliveries,
    media_metadatas,
    media_thumbnails,
    room_aliases,
    room_servers,
    room_state_deltas,
    room_state_fields,
    room_state_frames,
    room_state_points,
    room_tags,
    room_threads,
    room_users,
    rooms,
    server_signing_keys,
    stats_monthly_active_users,
    stats_room_currents,
    stats_user_daily_visits,
    thread_users,
    threads,
    threepid_guests,
    threepid_id_servers,
    threepid_validation_sessions,
    threepid_validation_tokens,
    user_access_tokens,
    user_datas,
    user_dehydrated_devices,
    user_devices,
    user_filters,
    user_ignores,
    user_openid_tokens,
    user_passwords,
    user_presences,
    user_profiles,
    user_pushers,
    user_refresh_tokens,
    user_registration_tokens,
    user_sessions,
    user_threepids,
    user_uiaa_datas,
    users,
);
