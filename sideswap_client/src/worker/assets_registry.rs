use std::collections::HashMap;

use sideswap_api::{Asset, AssetId, IssuancePrevout, Ticker};
use sideswap_common::env::Env;
use sideswap_types::{asset_precision::AssetPrecision, proxy_address::ProxyAddress};

pub fn init(registry_path: &std::path::Path) {
    if let Err(error) = gdk_registry::init(registry_path) {
        match error {
            gdk_registry::Error::AlreadyInitialized => {}
            _ => panic!("gdk_registry init failed: {}", error),
        }
    }
}

pub fn refresh(env: Env, xpub: bitcoin::bip32::Xpub, proxy: Option<ProxyAddress>) {
    std::thread::spawn(move || {
        gdk_registry::refresh_assets(gdk_registry::RefreshAssetsParams {
            assets: true,
            icons: true,
            xpub: Some(xpub),
            config: get_registry_config(env, &proxy),
        })
    });
}

pub fn get_assets(
    env: Env,
    xpub: bitcoin::bip32::Xpub,
    asset_ids: Vec<AssetId>,
    proxy: &Option<ProxyAddress>,
) -> Result<Vec<Asset>, anyhow::Error> {
    let xpub = gdk_common::bitcoin::bip32::Xpub::decode(&xpub.encode()).unwrap();
    let loaded_assets = gdk_registry::get_assets(gdk_registry::GetAssetsParams {
        assets_id: Some(asset_ids.clone()),
        xpub: Some(xpub),
        config: get_registry_config(env, proxy),
        names: None,
        tickers: None,
        category: None,
    })?;

    let result = asset_ids
        .iter()
        .filter_map(|asset_id| {
            loaded_assets.assets.get(asset_id).map(|v| {
                let icon = loaded_assets.icons.get(asset_id).cloned();
                let default_ticker = || format!("{:0.4}", &asset_id.to_string());
                Asset {
                    asset_id: *asset_id,
                    name: v.name.clone(),
                    ticker: Ticker(v.ticker.clone().unwrap_or_else(default_ticker)),
                    icon,
                    precision: AssetPrecision::new(v.precision)
                        .expect("only precision in the 0..8 range is allowed in the GDK registry"),
                    icon_url: None,
                    instant_swaps: Some(false),
                    domain: v.entity["domain"].as_str().map(|s| s.to_owned()),
                    domain_agent: None,
                    domain_agent_link: None,
                    always_show: None,
                    issuance_prevout: Some(IssuancePrevout {
                        txid: v.issuance_prevout.txid,
                        vout: v.issuance_prevout.vout,
                    }),
                    issuer_pubkey: Some(v.issuer_pubkey.clone()),
                    contract: Some(v.contract.clone()),
                    market_type: Some(sideswap_api::MarketType::Token),
                    server_fee: None,
                    amp_asset_restrictions: None,
                    payjoin: None,
                }
            })
        })
        .collect();
    Ok(result)
}

fn get_registry_config(env: Env, proxy: &Option<ProxyAddress>) -> gdk_registry::Config {
    let network = match env.d().network {
        sideswap_common::network::Network::Liquid => gdk_registry::ElementsNetwork::Liquid,
        sideswap_common::network::Network::LiquidTestnet => {
            gdk_registry::ElementsNetwork::LiquidTestnet
        }
        sideswap_common::network::Network::Regtest => {
            gdk_registry::ElementsNetwork::ElementsRegtest
        }
    };
    gdk_registry::Config {
        // Must be in this format - socks5://{ip}:{port}
        proxy: proxy.as_ref().map(ToString::to_string),
        url: env.nd().asset_registry_url.to_owned(),
        network,
        custom_headers: HashMap::new(),
    }
}
