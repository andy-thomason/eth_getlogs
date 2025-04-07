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

/// These interfaces come from github
/// 
/// https://github.com/Uniswap/v2-core/tree/master/contracts
/// 
sol!(
    /// All tokens support these calls and events.
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

    /// All uniswap v2 pairs support these.
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

/// A "catch all" error.
type BDE = Box<dyn std::error::Error>;

#[tokio::main]
async fn main() -> Result<(), BDE> {
    let url = std::env::var("ALCHEMY_URL")?.parse()?;
    let provider = ProviderBuilder::new().on_http(url);

    let mut erc20s : HashMap<Address, Option<Erc20>> = HashMap::new();
    let mut pairs : HashMap<Address, Option<Pair>> = HashMap::new();

    // See https://docs.rs/alloy/latest/alloy/providers/trait.Provider.html
    let latest_block = provider.get_block_number().await?;

    let swap_hash = IUniswapV2Pair::Sync::SIGNATURE_HASH;
    let filter = Filter::new()
        .from_block(latest_block-20)
        .to_block(latest_block)
        .event_signature(swap_hash);

    let logs = provider.get_logs(&filter).await?;

    use std::io::Write;
    println!("symbol_0\tsymbol_1\tprice");

    for l in logs {
        let transaction_hash = l.transaction_hash.unwrap_or_default();
        let sync : alloy::rpc::types::Log<IUniswapV2Pair::Sync>  = l.log_decode().unwrap();
        let block_number = sync.block_number.unwrap_or_default();
        let contract_address = sync.address();
        let IUniswapV2Pair::Sync{ reserve0, reserve1 } = sync.inner.deref();

        let pair = get_pair(&provider, &mut erc20s, &mut pairs, contract_address).await?;
        if let Some(pair) = pair {
            if let (Some(e0), Some(e1)) = (&pair.erc20_0, &pair.erc20_1) {
                /// Each token has a symbol.
                let symbol_0 = &e0.symbol;
                let symbol_1 = &e1.symbol;

                /// The decimals represent the number of digits in each price.
                let decimals_0 = e0.decimals as i32;
                let decimals_1 = e1.decimals as i32;

                /// The price is the ratio of the reserves.
                let r0 : f64 = reserve0.try_into().unwrap();
                let r1 : f64 = reserve1.try_into().unwrap();
                let price = r0 * 10_f64.powi(decimals_1-decimals_0) / r1;

                println!("{symbol_0}\t{symbol_1}\t{price}\t{transaction_hash}");
            }
        }

    }

    Ok(())
}

/// Make a call to the ethereum node via the provider.
/// 
/// This executes contract code to get a result.
async fn do_call<P : Provider, Call : SolCall>(provider: &P, call: Call, address: Address) -> Result<Option<Call::Return>, BDE> {
    let input = call.abi_encode();
    let tx = TransactionRequest::default()
        .to(address)
        .input(input.into());
    let Ok(res) = provider.call(tx).await else { return Ok(None) };
    
    Ok(Some(Call::abi_decode_returns(&res, false)?))
}


/// Fetch ERC20 token data.
/// 
/// This executes contract code to get a result.
async fn get_erc20<P : Provider>(provider: &P, erc20s: &mut HashMap<Address, Option<Erc20>>, address: Address) -> Result<Option<Erc20>, BDE> {
    if let Some(erc20) = erc20s.get(&address) {
        // Seen this one before.
        return Ok(erc20.clone());
    }

    let Some(decimals) = do_call(provider, IERC20::decimalsCall{}, address).await? else { return Ok(None) };
    let decimals = decimals._0;

    let Some(symbol) = do_call(provider, IERC20::symbolCall{}, address).await? else { return Ok(None) };
    let symbol = symbol._0.into();

    let erc20 = Some(Erc20 {
        decimals,
        symbol,
    });

    erc20s.insert(address, erc20.clone());

    Ok(erc20)
}

/// Fetch pair information for a swap.
/// 
async fn get_pair<P : Provider>(provider: &P, erc20s: &mut HashMap<Address, Option<Erc20>>, pairs : &mut HashMap<Address, Option<Pair>>, address: Address) -> Result<Option<Pair>, BDE> {
    if let Some(pair) = pairs.get(&address) {
        // Seen this one before.
        return Ok(pair.clone());
    }

    let Some(token0) = do_call(provider, IUniswapV2Pair::token0Call{}, address).await? else { return Ok(None) };
    let token0 = token0._0;

    let Some(token1) = do_call(provider, IUniswapV2Pair::token1Call{}, address).await? else { return Ok(None) };
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

