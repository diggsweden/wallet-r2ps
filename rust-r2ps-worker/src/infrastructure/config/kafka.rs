use dotenv_config::EnvConfig;

#[derive(Debug, EnvConfig, Clone)]
pub struct KafkaConfig {
    #[env_config(
        name = "BOOTSTRAP_SERVERS",
        default = "localhost:9092,localhost:9093,localhost:9094"
    )]
    pub bootstrap_servers: String,

    #[env_config(default = "v4")]
    pub broker_address_family: String,

    #[env_config(name = "GROUP_ID", default = "rust-grp")]
    pub group_id: String,

    #[env_config(name = "GROUP_INSTANCE_ID", default = "consumer-1")]
    pub group_instance_id: String,
}
