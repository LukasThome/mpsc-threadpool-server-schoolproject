use std::collections::{HashMap, VecDeque};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use rand::{Rng, prelude::IteratorRandom};
use clap::{Arg, Command};

fn main() {
    let matches = Command::new("Bank Server Simulation")
        .about("Simulates a bank server handling client requests with worker threads")
        .override_usage("cargo run -- --workers <workers> --clients <clients> --accounts <accounts> --requests <requests> --deposit-sleep <ms> --transfer-sleep <ms> --balance-sleep <ms> --client-sleep <ms>")
        .arg(Arg::new("workers").long("workers").default_value("5").value_parser(clap::value_parser!(usize)).help("Number of worker threads in the thread pool"))
        .arg(Arg::new("clients").long("clients").default_value("10").value_parser(clap::value_parser!(usize)).help("Number of client threads generating requests"))
        .arg(Arg::new("accounts").long("accounts").default_value("100").value_parser(clap::value_parser!(u32)).help("Number of bank accounts"))
        .arg(Arg::new("requests").long("requests").default_value("100").value_parser(clap::value_parser!(usize)).help("Number of requests each client thread will generate"))
        .arg(Arg::new("deposit_sleep").long("deposit-sleep").default_value("100").value_parser(clap::value_parser!(u64)).help("Sleep duration (in ms) for each deposit operation"))
        .arg(Arg::new("transfer_sleep").long("transfer-sleep").default_value("150").value_parser(clap::value_parser!(u64)).help("Sleep duration (in ms) for each transfer operation"))
        .arg(Arg::new("balance_sleep").long("balance-sleep").default_value("200").value_parser(clap::value_parser!(u64)).help("Sleep duration (in ms) for each balance snapshot operation"))
        .arg(Arg::new("client_sleep").long("client-sleep").default_value("50").value_parser(clap::value_parser!(u64)).help("Sleep duration (in ms) between each client request"))
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

    let incoming_requests = Arc::new(Mutex::new(VecDeque::new()));

    let request_queue = Arc::new(RequestQueue {
        queue: Mutex::new(VecDeque::new()),
        condvar: Condvar::new(),
        no_more_clients: AtomicBool::new(false),
    });

    // server thread
    let server_handle = {
        let incoming_requests = Arc::clone(&incoming_requests);
        let request_queue = Arc::clone(&request_queue);
        thread::spawn(move || {
            let mut client_operations_counter = 0;
            let mut empty_since = None;
            while !(empty_since.map_or(false, |start: Instant| start.elapsed() >= Duration::from_secs(5))
                    && request_queue.no_more_clients.load(Ordering::SeqCst) && incoming_requests.lock().unwrap().is_empty()) 
            {
                let operation = incoming_requests.lock().unwrap().pop_front();
                if let Some(operation) = operation {
                    let mut queue = request_queue.queue.lock().unwrap();
                    queue.push_back(operation);
                    client_operations_counter += 1;

                    if client_operations_counter % 10 == 0 && client_operations_counter > 0 {
                        queue.push_back(Operation::GeneralBalance);
                    }

                    request_queue.condvar.notify_one();
                    empty_since = None;
                } else if request_queue.queue.lock().unwrap().is_empty() {
                    empty_since = empty_since.or_else(|| Some(Instant::now()));
                } else {
                    empty_since = None;
                }
            }
            thread::sleep(Duration::from_millis(100));
            println!("Finished processing all client requests.");
        })
    };

    // worker threads
    let workers_running = Arc::new(AtomicBool::new(true));
    let worker_handles: Vec<_> = (0..num_workers).map(|id| {
        let (bank, request_queue, workers_running) = (
            Arc::clone(&bank), Arc::clone(&request_queue), Arc::clone(&workers_running), 
        );
        thread::spawn(move || -> f64 {
            let mut local_net_deposits = 0.0;
            while workers_running.load(Ordering::SeqCst)
            {
                let operation = {
                    let mut queue = request_queue.queue.lock().unwrap();
                    while queue.is_empty() && !request_queue.no_more_clients.load(Ordering::SeqCst)
                    {
                        queue = request_queue.condvar.wait_timeout(queue, Duration::from_secs(1)).unwrap().0;
                    }
                    queue.pop_front()
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
                            let elapsed = Instant::now().elapsed();
                            let (minutes, seconds, milliseconds) = (elapsed.as_secs() / 60,elapsed.as_secs() % 60, elapsed.subsec_millis());
                            let snapshot: Vec<(u32, f64)> = {
                                bank.accounts.iter()
                                    .map(|(id, account)| {(*id, *account.balance.lock().unwrap())}).collect()
                            };
                            println!("\n--- General Balance Snapshot {:02}:{:02}.{:03} ---", minutes, seconds, milliseconds);
                            snapshot.iter().for_each(|(id, balance)| {println!("Account {}: Balance {:.2}", id, balance);});
                            println!("--------------------------------\n");
                        }
                    }
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
        let incoming_requests = Arc::clone(&incoming_requests);
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
                incoming_requests.lock().unwrap().push_back(operation);
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
    queue: Mutex<VecDeque<Operation>>,
    condvar: Condvar,
    no_more_clients: AtomicBool,
}
