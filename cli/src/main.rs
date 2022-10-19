use std::ops::Deref;
use std::str::FromStr;
use clap::{Arg, ArgMatches, Command};
use solana_cli_config::Config;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcSendTransactionConfig};
use solana_program::pubkey::Pubkey;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::signature::{Keypair, read_keypair_file};
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use spl_token_2022::state::{Account, Mint};
use spl_token_2022::extension::{ExtensionType, StateWithExtensions};
use spl_token_2022::extension::transfer_fee::{TransferFeeAmount, TransferFeeConfig};

pub(crate) type Error = Box<dyn std::error::Error>;

fn main() {
    let cmd = Command::new("t2022-cli")
        .bin_name("t2022-cli")
        .subcommand_required(true)
        .subcommand(
            Command::new("create-token")
                .arg(Arg::new("TOKEN_KEYPAIR").required(false).index(1))
        )
        .subcommand(
            Command::new("mint")
                .arg(Arg::new("TOKEN_ADDRESS").required(true).index(1))
                .arg(Arg::new("TOKEN_AMOUNT").required(true).index(2))
        )
        .subcommand(
            Command::new("transfer")
                .arg(Arg::new("TOKEN_ADDRESS").required(true).index(1))
                .arg(Arg::new("TOKEN_AMOUNT").required(true).index(2))
                .arg(Arg::new("RECIPIENT_ADDRESS").required(true).index(3))
        )
        .subcommand(
            Command::new("account-info")
                .arg(Arg::new("TOKEN_ACCOUNT_ADDRESS").required(true).index(1))
        )
        .subcommand(
            Command::new("mint-info")
                .arg(Arg::new("MINT_ACCOUNT_ADDRESS").required(true).index(1))
        );

    let matches = cmd.get_matches();

    let config_file = solana_cli_config::CONFIG_FILE.as_ref().unwrap();
    let cli_config = Config::load(config_file).unwrap();

    let rpc_client = RpcClient::new_with_commitment(&cli_config.json_rpc_url, CommitmentConfig::confirmed());

    let payer = read_keypair_file(cli_config.keypair_path).unwrap();

    let result = match matches.subcommand() {
        Some(("create-token", matches)) => {
            create_token(rpc_client, payer, matches)
        }
        Some(("mint", matches)) => {
            mint(rpc_client, payer, matches)
        }
        Some(("transfer", matches)) => {
            transfer(rpc_client, payer, matches)
        }
        Some(("account-info", matches)) => {
            account_info(rpc_client, matches)
        }
        Some(("mint-info", matches)) => {
            mint_info(rpc_client, matches)
        }
        _ => unreachable!(),
    };

    if let Err(error) = result {
        println!("{}", error);
    }
}


fn create_token(rpc_client: RpcClient, payer: Keypair, matches: &ArgMatches) -> Result<(), Error> {
    let mint_keypair = matches.value_of("TOKEN_KEYPAIR")
        .map_or(Ok(Keypair::new()), |path| { read_keypair_file(path) })?;
    println!("Mint: {:?}", mint_keypair.pubkey());

    let space = ExtensionType::get_account_len::<Mint>(&[ExtensionType::TransferFeeConfig]);

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
        u64::MAX,
    )?);


    instructions.push(spl_token_2022::instruction::initialize_mint2(
        &spl_token_2022::id(),
        &mint_keypair.pubkey(),
        &payer.pubkey(),
        None,
        6,
    )?);


    let transaction = Transaction::new_signed_with_payer(
        instructions.deref(),
        Some(&payer.pubkey()),
        &[&payer, &mint_keypair],
        rpc_client.get_latest_blockhash()?,
    );

    send_tx(&transaction, &rpc_client);
    Ok(())
}


fn mint(rpc_client: RpcClient, payer: Keypair, matches: &ArgMatches) -> Result<(), Error> {
    let token_address = Pubkey::from_str(matches.value_of("TOKEN_ADDRESS").unwrap())
        .map_err(|_| format!("Invalid token address"))?;

    let state_data = rpc_client.get_account_data(&token_address)?;
    let mint_state = StateWithExtensions::<Mint>::unpack(state_data.as_ref())?.base;

    let amount = spl_token_2022::ui_amount_to_amount(
        f64::from_str(matches.value_of("TOKEN_AMOUNT").unwrap())?,
        mint_state.decimals);

    let token_account = spl_associated_token_account::get_associated_token_address_with_program_id(&payer.pubkey(), &token_address, &spl_token_2022::id());
    println!("Target token account: {:?}", token_account);

    let mut instructions = vec![];


    if rpc_client.get_account(&token_account).is_err() {
        instructions.push(spl_associated_token_account::instruction::create_associated_token_account(
            &payer.pubkey(),
            &payer.pubkey(),
            &token_address,
            &spl_token_2022::id(),
        ));
    }

    instructions.push(spl_token_2022::instruction::mint_to(
        &spl_token_2022::id(),
        &token_address,
        &token_account,
        &payer.pubkey(),
        &[],
        amount,
    )?);

    let transaction = Transaction::new_signed_with_payer(
        instructions.deref(),
        Some(&payer.pubkey()),
        &[&payer],
        rpc_client.get_latest_blockhash()?,
    );

    send_tx(&transaction, &rpc_client);
    Ok(())
}

fn transfer(rpc_client: RpcClient, payer: Keypair, matches: &ArgMatches) -> Result<(), Error> {
    let token_address = Pubkey::from_str(matches.value_of("TOKEN_ADDRESS").unwrap())
        .map_err(|_| format!("Invalid token address"))?;

    let recipient_address = Pubkey::from_str(matches.value_of("RECIPIENT_ADDRESS").unwrap())
        .map_err(|_| format!("Invalid token address"))?;

    let state_data = rpc_client.get_account_data(&token_address)?;
    let mint_state = StateWithExtensions::<Mint>::unpack(state_data.as_ref())?.base;

    let amount = spl_token_2022::ui_amount_to_amount(
        f64::from_str(matches.value_of("TOKEN_AMOUNT").unwrap())?,
        mint_state.decimals);

    let source_token_account = spl_associated_token_account::get_associated_token_address_with_program_id(&payer.pubkey(), &token_address, &spl_token_2022::id());

    let recipient_token_account = spl_associated_token_account::get_associated_token_address_with_program_id(&recipient_address, &token_address, &spl_token_2022::id());
    println!("Target token account: {:?}", recipient_token_account);

    let mut instructions = vec![];

    if rpc_client.get_account(&recipient_token_account).is_err() {
        instructions.push(spl_associated_token_account::instruction::create_associated_token_account(
            &payer.pubkey(),
            &recipient_address,
            &token_address,
            &spl_token_2022::id(),
        ));
    }

    instructions.push(spl_token_2022::instruction::transfer_checked(
        &spl_token_2022::id(),
        &source_token_account,
        &token_address,
        &recipient_token_account,
        &payer.pubkey(),
        &[],
        amount,
        mint_state.decimals,
    )?);

    let transaction = Transaction::new_signed_with_payer(
        instructions.deref(),
        Some(&payer.pubkey()),
        &[&payer],
        rpc_client.get_latest_blockhash()?,
    );

    send_tx(&transaction, &rpc_client);
    Ok(())
}

fn account_info(rpc_client: RpcClient, matches: &ArgMatches) -> Result<(), Error> {
    let address = Pubkey::from_str(matches.value_of("TOKEN_ACCOUNT_ADDRESS").unwrap())
        .map_err(|_| format!("Invalid token account address"))?;

    let state_data = rpc_client.get_account_data(&address)?;
    let state = StateWithExtensions::<Account>::unpack(state_data.as_ref())?;
    println!("{:?}", state.base);

    let extensions = state.get_extension_types()?;
    println!("Extensions: {:?}", extensions);

    if extensions.contains(&ExtensionType::TransferFeeAmount) {
        let transfer_fee_amount = state.get_extension::<TransferFeeAmount>()?;
        println!("Withheld transfer fee amount: {}", u64::from(transfer_fee_amount.withheld_amount));
    }
    Ok(())
}

fn mint_info(rpc_client: RpcClient, matches: &ArgMatches) -> Result<(), Error> {
    let address = Pubkey::from_str(matches.value_of("MINT_ACCOUNT_ADDRESS").unwrap())
        .map_err(|_| format!("Invalid token account address"))?;

    let state_data = rpc_client.get_account_data(&address)?;
    let state = StateWithExtensions::<Mint>::unpack(state_data.as_ref())?;
    println!("{:?}", state.base);

    let extensions = state.get_extension_types()?;
    println!("Extensions: {:?}", extensions);

    if extensions.contains(&ExtensionType::TransferFeeConfig) {
        let transfer_fee_config = state.get_extension::<TransferFeeConfig>()?;
        println!("{:?}", transfer_fee_config);
    }

    Ok(())
}


fn send_tx(transaction: &Transaction, rpc_client: &RpcClient) {
    let mut config = RpcSendTransactionConfig::default();
    config.skip_preflight = true;

    let transaction_result = rpc_client.send_transaction_with_config(&transaction, config);

    println!("Transaction {:?}", transaction_result);
}