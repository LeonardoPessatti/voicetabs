pub mod backend;
pub mod client;
pub mod framing;
pub mod keyring;
pub mod local;
pub mod openai;
pub mod protocol;
pub mod status;
pub mod supervisor;

pub use backend::{BackendError, SttBackend, TranscribeRequest};
pub use local::LocalSttBackend;
pub use status::{SttStatus, SttStatusHandle};
pub use supervisor::{SttSupervisor, SupervisorConfig, SupervisorError};
