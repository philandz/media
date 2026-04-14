pub mod pb {
    pub mod common {
        pub mod base {
            tonic::include_proto!("common.base");
        }
    }
    pub mod service {
        pub mod media {
            tonic::include_proto!("service.media");
        }
    }
    pub mod shared {
        pub mod media {
            tonic::include_proto!("shared.media");
        }
    }
}

pub mod handler;
pub mod manager;
