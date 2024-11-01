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
    let server_running = Arc::new(AtomicBool::new(true));

    // server thread
    let server_handle = {
        let request_queue = Arc::clone(&request_queue);
        let server_running = Arc::clone(&server_running);
        let operation_count = Arc::clone(&operation_count);
        thread::spawn(move || {
            let (mut last_checked_count, mut empty_since) = (0, None);
            while server_running.load(Ordering::SeqCst) {
                if operation_count.load(Ordering::SeqCst) >= last_checked_count + 10 {
                    request_queue.queue.lock().unwrap().push(Request { operation: Operation::GeneralBalance });
                    request_queue.condvar.notify_one();
                    last_checked_count += 10;
                }
                if request_queue.queue.lock().unwrap().is_empty() && request_queue.no_more_clients.load(Ordering::SeqCst) {
                    empty_since = match empty_since {
                        None => Some(Instant::now()),
                        Some(start) if start.elapsed() >= Duration::from_secs(5) => break,
                        _ => empty_since,
                    };
                } else {
                    empty_since = None;
                }
                thread::sleep(Duration::from_millis(100));
            }
            server_running.store(false, Ordering::SeqCst);
            println!("Finished processing all client requests.");
        })
    };

    // worker threads
    let worker_running = Arc::new(AtomicBool::new(true));
    let worker_handles: Vec<_> = (0..num_workers).map(|id| {
        let (bank, request_queue, worker_running, operation_count) = (Arc::clone(&bank), Arc::clone(&request_queue), Arc::clone(&worker_running), Arc::clone(&operation_count));
        thread::spawn(move || {
            while worker_running.load(Ordering::SeqCst) {
                let request = {
                    let mut queue = request_queue.queue.lock().unwrap();
                    while queue.is_empty() && !request_queue.no_more_clients.load(Ordering::SeqCst) {
                        queue = request_queue.condvar.wait_timeout(queue, Duration::from_secs(1)).unwrap().0;
                    }
                    queue.pop()
                };
                if let Some(request) = request {
                    println!("Worker {} is now executing.", id);
                    match request.operation {
                        Operation::Deposit { account_id, amount } => {
                            thread::sleep(bank.deposit_sleep);
                            if let Some(account) = bank.accounts.get(&account_id) {
                                *account.balance.lock().unwrap() += amount;
                                println!("Deposit: Account {}: Amount {:.2}", account_id, amount);
                            }
                        }
                        Operation::Transfer { from_account_id, to_account_id, amount } if from_account_id != to_account_id => {
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
                            println!("\n--- General Balance Snapshot ---");
                            bank.accounts.iter().for_each(|(id, account)| println!("Account {}: Balance {:.2}", id, *account.balance.lock().unwrap()));
                            println!("--------------------------------\n");
                        }
                        _ => {}
                    }
                    operation_count.fetch_add(1, Ordering::SeqCst);
                    println!("Worker {} is now idle.", id);
                } else if request_queue.no_more_clients.load(Ordering::SeqCst) {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
            println!("Worker {} is terminating.", id);
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
                request_queue.queue.lock().unwrap().push(Request { operation });
                request_queue.condvar.notify_one();
                thread::sleep(client_sleep);
            });
        })
    }).collect();

    // graceful shutdown
    client_handles.into_iter().for_each(|handle| handle.join().unwrap());
    request_queue.no_more_clients.store(true, Ordering::SeqCst);
    server_handle.join().unwrap();
    worker_running.store(false, Ordering::SeqCst);
    worker_handles.into_iter().for_each(|handle| handle.join().unwrap());

    println!("Total client requests processed: {}", num_clients * requests_per_client);
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

struct Request {
    operation: Operation,
}

struct RequestQueue {
    queue: Mutex<Vec<Request>>,
    condvar: Condvar,
    no_more_clients: AtomicBool,
}