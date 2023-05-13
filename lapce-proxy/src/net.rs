pub struct Client {}

impl Client {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(proxy: String) -> anyhow::Result<reqwest::blocking::Client> {
        let mut client = reqwest::blocking::Client::builder();
        if !proxy.is_empty() {
            let proxy = reqwest::Proxy::all(proxy)?;
            client = client.proxy(proxy);
        }
        let client = client.build()?;

        Ok(client)
    }
}
