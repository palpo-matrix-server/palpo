#[macro_export]
macro_rules! join_path {
    ($($part:expr),+) => {
        {
            let mut p = std::path::PathBuf::new();
            $(
                p.push($part);
            )*
            path_slash::PathBufExt::to_slash_lossy(&p).to_string()
        }
    }
}

#[macro_export]
macro_rules! diesel_exists {
    ($query:expr, $conn:expr) => {{
        // tracing::info!( sql = %debug_query!(&$query), "diesel_exists");
        diesel::select(diesel::dsl::exists($query)).get_result::<bool>($conn)
    }};
    ($query:expr, $default:expr, $conn:expr) => {{
        // tracing::info!( sql = debug_query!(&$query), "diesel_exists");
        diesel::select(diesel::dsl::exists($query))
            .get_result::<bool>($conn)
            .unwrap_or($default)
    }};
}

#[macro_export]
macro_rules! print_query {
    ($query:expr) => {
        println!("{}", diesel::debug_query::<diesel::pg::Pg, _>($query));
    };
}

#[macro_export]
macro_rules! debug_query {
    ($query:expr) => {{
        format!("{}", diesel::debug_query::<diesel::pg::Pg, _>($query))
    }};
}
