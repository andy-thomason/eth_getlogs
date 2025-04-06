#![allow(warnings)]

use std::collections::HashMap;
use std::ops::Deref;
use alloy::primitives::Address;
use alloy::providers::Provider;
use alloy::rpc::types::{Filter, TransactionRequest};
use alloy::sol_types::{SolCall, SolEvent};
use alloy::{providers::ProviderBuilder, sol};

#[derive(Debug, Clone)]
struct Erc20 {
    decimals: u8,
    symbol: Box<str>,
}

#[derive(Debug, Clone)]
struct Pair {
    token0: Address,
    token1: Address,
    erc20_0: Option<Erc20>,
    erc20_1: Option<Erc20>,
}

macro_rules! do_call {
    ($dest : ident, $call: ty) => {
        let call = $call{};
        let input = call.abi_encode();
        let tx = TransactionRequest::default()
            .to(address)
            .input(input.into());
        let Ok(res) = provider.call(tx).await else { return Ok(None) };
    
        let Ok(token0) = $call::abi_decode_returns(&res, false) else { return Ok(None) };
        let $dest = token0._0;
    }
}

sol!(
    interface IERC20 {
        event Approval(address indexed owner, address indexed spender, uint value);
        event Transfer(address indexed from, address indexed to, uint value);
    
        function name() external pure returns (string memory);
        function symbol() external pure returns (string memory);
        function decimals() external pure returns (uint8);
        function totalSupply() external view returns (uint);
        function balanceOf(address owner) external view returns (uint);
        function allowance(address owner, address spender) external view returns (uint);
    }

    interface IUniswapV2Pair {
        event Approval(address indexed owner, address indexed spender, uint value);
        event Transfer(address indexed from, address indexed to, uint value);
    
        function name() external pure returns (string memory);
        function symbol() external pure returns (string memory);
        function decimals() external pure returns (uint8);
        function totalSupply() external view returns (uint);
        function balanceOf(address owner) external view returns (uint);
        function allowance(address owner, address spender) external view returns (uint);
    
        function approve(address spender, uint value) external returns (bool);
        function transfer(address to, uint value) external returns (bool);
        function transferFrom(address from, address to, uint value) external returns (bool);
    
        function DOMAIN_SEPARATOR() external view returns (bytes32);
        function PERMIT_TYPEHASH() external pure returns (bytes32);
        function nonces(address owner) external view returns (uint);
    
        function permit(address owner, address spender, uint value, uint deadline, uint8 v, bytes32 r, bytes32 s) external;
    
        event Mint(address indexed sender, uint amount0, uint amount1);
        event Burn(address indexed sender, uint amount0, uint amount1, address indexed to);
        event Swap(
            address indexed sender,
            uint amount0In,
            uint amount1In,
            uint amount0Out,
            uint amount1Out,
            address indexed to
        );
        event Sync(uint112 reserve0, uint112 reserve1);
    
        function MINIMUM_LIQUIDITY() external pure returns (uint);
        function factory() external view returns (address);
        function token0() external view returns (address);
        function token1() external view returns (address);
        function getReserves() external view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast);
        function price0CumulativeLast() external view returns (uint);
        function price1CumulativeLast() external view returns (uint);
        function kLast() external view returns (uint);
    
        function mint(address to) external returns (uint liquidity);
        function burn(address to) external returns (uint amount0, uint amount1);
        function swap(uint amount0Out, uint amount1Out, address to, bytes calldata data) external;
        function skim(address to) external;
        function sync() external;
    
        function initialize(address, address) external;
    }
);

type BDE = Box<dyn std::error::Error>;

#[tokio::main]
async fn main() -> Result<(), BDE> {
    let url = std::env::var("ALCHEMY_URL")?.parse()?;
    let provider = ProviderBuilder::new().on_http(url);

    let mut erc20s : HashMap<Address, Option<Erc20>> = HashMap::new();
    let mut pairs : HashMap<Address, Option<Pair>> = HashMap::new();


    // See https://docs.rs/alloy/latest/alloy/providers/trait.Provider.html
    let latest_block = provider.get_block_number().await?;

    println!("latest_block={latest_block}");

    let swap_hash = IUniswapV2Pair::Sync::SIGNATURE_HASH;
    let block_start = 12000000;
    let filter = Filter::new()
        .from_block(block_start)
        .to_block(block_start+999)
        .event_signature(swap_hash);

    let logs = provider.get_logs(&filter).await?;

    let mut out = std::fs::File::create("/tmp/result.tsv")?;
    use std::io::Write;
    // writeln!(out, "block_number\tcontract_address\tsender\tamount0In\tamount1In\tamount0Out\tamount1Out\tto\tsender_decimals\tsender_symbol\tto_decimals\tto_symbol")?;

    for l in logs {
        let sync : alloy::rpc::types::Log<IUniswapV2Pair::Sync>  = l.log_decode().unwrap();
        writeln!(out, "{:?}", l.transaction_hash)?;
        let block_number = sync.block_number.unwrap_or_default();
        let contract_address = sync.address();
        let IUniswapV2Pair::Sync{ reserve0, reserve1 } = sync.inner.deref();

        let pair = get_pair(&provider, &mut erc20s, &mut pairs, contract_address).await?;
        if let Some(pair) = pair {
            if let (Some(e0), Some(e1)) = (&pair.erc20_0, &pair.erc20_1) {
                let symbol_0 = &e0.symbol;
                let symbol_1 = &e1.symbol;
                let decimals_0 = e0.decimals as i32;
                let decimals_1 = e1.decimals as i32;
                let r0 : f64 = reserve0.try_into().unwrap();
                let r1 : f64 = reserve1.try_into().unwrap();
                let price = r0 * 10_f64.powi(decimals_1-decimals_0) / r1;
                writeln!(out, "price\t{symbol_0}/{symbol_1}\t{price}")?;
            }
        }

    }

    Ok(())
}


async fn get_erc20<P : Provider>(provider: &P, erc20s: &mut HashMap<Address, Option<Erc20>>, address: Address) -> Result<Option<Erc20>, BDE> {
    let call = IERC20::decimalsCall{};
    let input = call.abi_encode();
    let tx = TransactionRequest::default()
        .to(address)
        .input(input.into());
    let Ok(res) = provider.call(tx).await else { return Ok(None) };

    let Ok(decimals) = IERC20::decimalsCall::abi_decode_returns(&res, false) else { return Ok(None) };
    let decimals = decimals._0;

    let call = IERC20::symbolCall{};
    let input = call.abi_encode();
    let tx = TransactionRequest::default()
        .to(address)
        .input(input.into());
    let Ok(res) = provider.call(tx).await else { return Ok(None) };

    let Ok(symbol) = IERC20::symbolCall::abi_decode_returns(&res, false) else { return Ok(None) };
    let symbol = symbol._0.into();

    Ok(Some(Erc20 {
        decimals,
        symbol,
    }))
}

async fn get_pair<P : Provider>(provider: &P, erc20s: &mut HashMap<Address, Option<Erc20>>, pairs : &mut HashMap<Address, Option<Pair>>, address: Address) -> Result<Option<Pair>, BDE> {
    if let Some(pair) = pairs.get(&address) {
        return Ok(pair.clone());
    }
    let call = IUniswapV2Pair::token0Call{};
    let input = call.abi_encode();
    let tx = TransactionRequest::default()
        .to(address)
        .input(input.into());
    let Ok(res) = provider.call(tx).await else { return Ok(None) };

    let Ok(token0) = IUniswapV2Pair::token0Call::abi_decode_returns(&res, false) else { return Ok(None) };
    let token0 = token0._0;

    let call = IUniswapV2Pair::token1Call{};
    let input = call.abi_encode();
    let tx = TransactionRequest::default()
        .to(address)
        .input(input.into());
    let Ok(res) = provider.call(tx).await else { return Ok(None) };

    let Ok(token1) = IUniswapV2Pair::token1Call::abi_decode_returns(&res, false) else { return Ok(None) };
    let token1 = token1._0;

    let erc20_0 = get_erc20(provider, erc20s, token0).await?;
    let erc20_1 = get_erc20(provider, erc20s, token1).await?;

    let pair = Some(Pair {
        token0,
        token1,
        erc20_0,
        erc20_1,
    });
    pairs.insert(address, pair.clone());
    Ok(pair)
}

