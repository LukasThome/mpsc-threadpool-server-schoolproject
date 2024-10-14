use std::collections::HashMap;
use std::sync::{atomic::{AtomicBool, AtomicUsize, Ordering}, Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use rand::{Rng, prelude::IteratorRandom};
use clap::{Arg, Command};

fn main() {
    let matches = Command::new("Bank Server Simulation")
        .version("1.0")
        .author("Your Name")
        .about("Simulates a bank server with client requests and worker threads")
        .arg(Arg::new("workers").long("workers").default_value("5").value_parser(clap::value_parser!(usize)))
        .arg(Arg::new("clients").long("clients").default_value("10").value_parser(clap::value_parser!(usize)))
        .arg(Arg::new("accounts").long("accounts").default_value("100").value_parser(clap::value_parser!(u32)))
        .arg(Arg::new("requests").long("requests").default_value("100").value_parser(clap::value_parser!(usize)))
        .arg(Arg::new("deposit_sleep").long("deposit-sleep").default_value("100").value_parser(clap::value_parser!(u64)))
        .arg(Arg::new("transfer_sleep").long("transfer-sleep").default_value("150").value_parser(clap::value_parser!(u64)))
        .arg(Arg::new("balance_sleep").long("balance-sleep").default_value("200").value_parser(clap::value_parser!(u64)))
        .arg(Arg::new("client_sleep").long("client-sleep").default_value("50").value_parser(clap::value_parser!(u64)))
        .get_matches();

    let (num_workers, num_clients, num_accounts, requests_per_client, deposit_sleep, transfer_sleep, balance_sleep, client_sleep) = (
        *matches.get_one::<usize>("workers").unwrap(),
        *matches.get_one::<usize>("clients").unwrap(),
        *matches.get_one::<u32>("accounts").unwrap(),
        *matches.get_one::<usize>("requests").unwrap(),
        Duration::from_millis(*matches.get_one::<u64>("deposit_sleep").unwrap()),
        Duration::from_millis(*matches.get_one::<u64>("transfer_sleep").unwrap()),
        Duration::from_millis(*matches.get_one::<u64>("balance_sleep").unwrap()),
        Duration::from_millis(*matches.get_one::<u64>("client_sleep").unwrap()),
    );

    let bank = Arc::new(Bank {
        accounts: (1..=num_accounts).map(|id| (id, Account { balance: Mutex::new(1000.0) })).collect(),
        deposit_sleep,
        transfer_sleep,
        balance_sleep,
    });

    let request_queue = Arc::new(RequestQueue {
        queue: Mutex::new(Vec::new()),
        condvar: Condvar::new(),
        no_more_clients: AtomicBool::new(false),
    });

    let operation_count = Arc::new(AtomicUsize::new(0));

    // server thread
    let server_handle = {
        let request_queue = Arc::clone(&request_queue);
        let operation_count = Arc::clone(&operation_count);
        thread::spawn(move || {
            let mut empty_since = None;
            while !(empty_since.map_or(false, |start: Instant| start.elapsed() >= Duration::from_secs(5))) {
                {
                    if operation_count.load(Ordering::SeqCst) % 10 == 0 {
                        request_queue.queue.lock().unwrap().push(Operation::GeneralBalance);
                        request_queue.condvar.notify_one();
                    }
                    if request_queue.queue.lock().unwrap().is_empty() && request_queue.no_more_clients.load(Ordering::SeqCst) {
                        empty_since = empty_since.or_else(|| Some(Instant::now()));
                    } else {
                        empty_since = None;
                    }
                }
                thread::sleep(Duration::from_millis(100));
            }
            println!("Finished processing all client requests.");
        })
    };

    // worker threads
    let workers_running = Arc::new(AtomicBool::new(true));
    let worker_handles: Vec<_> = (0..num_workers).map(|id| {
        let (bank, request_queue, workers_running, operation_count) = (Arc::clone(&bank), Arc::clone(&request_queue), Arc::clone(&workers_running), Arc::clone(&operation_count));
        thread::spawn(move || -> f64 {
            let mut local_net_deposits = 0.0;
            while workers_running.load(Ordering::SeqCst) {
                let operation = {
                    let mut queue = request_queue.queue.lock().unwrap();
                    while queue.is_empty() && !request_queue.no_more_clients.load(Ordering::SeqCst) {
                        queue = request_queue.condvar.wait_timeout(queue, Duration::from_secs(1)).unwrap().0;
                    }
                    queue.pop()
                };
                if let Some(operation) = operation {
                    println!("Worker {} is now executing.", id);
                    match operation {
                        Operation::Deposit { account_id, amount } => {
                            thread::sleep(bank.deposit_sleep);
                            if let Some(account) = bank.accounts.get(&account_id) {
                                *account.balance.lock().unwrap() += amount;
                                println!("Deposit: Account {}: Amount {:.2}", account_id, amount);
                                local_net_deposits += amount;
                            }
                        }
                        Operation::Transfer { from_account_id, to_account_id, amount } => {
                            thread::sleep(bank.transfer_sleep);
                            if let (Some(from_account), Some(to_account)) = (bank.accounts.get(&from_account_id), bank.accounts.get(&to_account_id)) {
                                let (mut from_balance, mut to_balance) = if from_account_id < to_account_id {
                                    (from_account.balance.lock().unwrap(), to_account.balance.lock().unwrap())
                                } else {
                                    (to_account.balance.lock().unwrap(), from_account.balance.lock().unwrap())
                                };
                                if *from_balance >= amount {
                                    *from_balance -= amount;
                                    *to_balance += amount;
                                    println!("Transfer: From Account {} to Account {}: Amount {:.2}", from_account_id, to_account_id, amount);
                                } else {
                                    eprintln!("Transfer failed: Account {} has insufficient funds.", from_account_id);
                                }
                            }
                        }
                        Operation::GeneralBalance => {
                            thread::sleep(bank.balance_sleep);
                            let snapshot: Vec<(u32, f64)> = {
                                bank.accounts.iter()
                                    .map(|(id, account)| {(*id, *account.balance.lock().unwrap())}).collect()
                            };
                            println!("\n--- General Balance Snapshot ---");
                            snapshot.iter().for_each(|(id, balance)| {println!("Account {}: Balance {:.2}", id, balance);});
                            println!("--------------------------------\n");
                        }
                    }
                    operation_count.fetch_add(1, Ordering::SeqCst);
                    println!("Worker {} is now idle.", id);
                } else if request_queue.no_more_clients.load(Ordering::SeqCst) {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
            println!("Worker {} is terminating.", id);
            local_net_deposits
        })
    }).collect();

    // client threads
    let client_handles: Vec<_> = (0..num_clients).map(|_| {
        let request_queue = Arc::clone(&request_queue);
        thread::spawn(move || {
            let mut rng = rand::thread_rng();
            (0..requests_per_client).for_each(|_| {
                let operation = if rng.gen_bool(0.5) {
                    Operation::Deposit {
                        account_id: rng.gen_range(1..=num_accounts),
                        amount: rng.gen_range(-100.0..100.0),
                    }
                } else {
                    let from_account_id = rng.gen_range(1..=num_accounts);
                    let to_account_id = (1..=num_accounts).filter(|&id| id != from_account_id).choose(&mut rng).unwrap();
                    Operation::Transfer {
                        from_account_id,
                        to_account_id,
                        amount: rng.gen_range(0.0..100.0),
                    }
                };
                request_queue.queue.lock().unwrap().push(operation);
                request_queue.condvar.notify_one();
                thread::sleep(client_sleep);
            });
        })
    }).collect();

    // graceful shutdown
    client_handles.into_iter().for_each(|handle| handle.join().unwrap());
    request_queue.no_more_clients.store(true, Ordering::SeqCst);
    server_handle.join().unwrap();
    workers_running.store(false, Ordering::SeqCst);
    let total_net_deposits: f64 = worker_handles.into_iter().map(|handle| handle.join().unwrap()).sum();
    println!("Total client requests processed: {}", num_clients * requests_per_client);
    let initial_total_balance = num_accounts as f64 * 1000.0;
    let expected_total_balance = initial_total_balance + total_net_deposits;
    let actual_total_balance: f64 = bank.accounts.values().map(|account| *account.balance.lock().unwrap()).sum();
    println!("Initial Total Balance: {:.2}", initial_total_balance);
    println!("Net Deposits: {:.2}", total_net_deposits);
    println!("Expected Total Balance: {:.2}", expected_total_balance);
    println!("Actual Total Balance: {:.2}", actual_total_balance);

    if (expected_total_balance - actual_total_balance).abs() < 0.01 {
        println!("Balances match. Simulation consistent.");
    } else {
        println!("Balances do not match. Simulation inconsistent!");
    }
}

struct Account {
    balance: Mutex<f64>,
}

struct Bank {
    accounts: HashMap<u32, Account>,
    deposit_sleep: Duration,
    transfer_sleep: Duration,
    balance_sleep: Duration,
}

#[derive(Clone)]
enum Operation {
    Deposit { account_id: u32, amount: f64 },
    Transfer { from_account_id: u32, to_account_id: u32, amount: f64 },
    GeneralBalance,
}

struct RequestQueue {
    queue: Mutex<Vec<Operation>>,
    condvar: Condvar,
    no_more_clients: AtomicBool,
}