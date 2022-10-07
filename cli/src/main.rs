use std::ops::Deref;
use clap::Command;
use solana_cli_config::Config;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcSendTransactionConfig, RpcTransactionConfig};
use solana_program::program_pack::Pack;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::signature::{Keypair, read_keypair_file};
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use spl_token_2022::state::Mint;
use spl_token_2022::extension::ExtensionType;
use spl_token_2022::extension::ExtensionType::TransferFeeConfig;

pub(crate) type Error = Box<dyn std::error::Error>;

fn main() {
    let cmd = Command::new("t2022-cli")
        .bin_name("t2022-cli")
        .subcommand_required(true)
        .subcommand(
            Command::new("create-token")
        );

    let matches = cmd.get_matches();

    let config_file = solana_cli_config::CONFIG_FILE.as_ref().unwrap();
    let cli_config = Config::load(config_file).unwrap();

    let rpc_client = RpcClient::new_with_commitment(&cli_config.json_rpc_url, CommitmentConfig::confirmed());

    let payer = read_keypair_file(cli_config.keypair_path).unwrap();

    let result = match matches.subcommand() {
        Some(("create-token", _)) => {
            create_token(rpc_client, payer)
        }
        _ => unreachable!(),
    };

    if let Err(error) = result {
        println!("{}", error);
    }
}


fn create_token(rpc_client: RpcClient, payer: Keypair) -> Result<(), Error> {

    let mint_keypair = Keypair::new();
    println!("Mint: {:?}", mint_keypair.pubkey());

    let space = ExtensionType::get_account_len::<Mint>(&[TransferFeeConfig]);

    let mut instructions = vec![];
    instructions.push(solana_program::system_instruction::create_account(
        &payer.pubkey(),
        &mint_keypair.pubkey(),
        rpc_client.get_minimum_balance_for_rent_exemption(space)?,
        space as u64,
        &spl_token_2022::id(),
    ));

    instructions.push(spl_token_2022::extension::transfer_fee::instruction::initialize_transfer_fee_config(
        &spl_token_2022::id(),
        &mint_keypair.pubkey(),
        Some(&payer.pubkey()),
        Some(&payer.pubkey()),
        123,
        u64::MAX
    )?);


    instructions.push(spl_token_2022::instruction::initialize_mint2(
        &spl_token_2022::id(),
        &mint_keypair.pubkey(),
        &payer.pubkey(),
        None,
        6
    )?);


    let transaction = Transaction::new_signed_with_payer(
        instructions.deref(),
        Some(&payer.pubkey()),
        &[&payer, &mint_keypair],
        rpc_client.get_latest_blockhash()?,
    );

    let mut config = RpcSendTransactionConfig::default();
    config.skip_preflight = true;
    let transaction_result = rpc_client.send_transaction_with_config(&transaction, config);

    //let transaction_result = rpc_client.send_and_confirm_transaction(&transaction);
    println!("Transaction {:?}", transaction_result);


    Ok(())
}