use std::collections::HashMap;
use std::sync::{atomic::{AtomicBool, AtomicUsize, Ordering}, Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use rand::Rng;
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

    let num_workers = *matches.get_one::<usize>("workers").unwrap();
    let num_clients = *matches.get_one::<usize>("clients").unwrap();
    let num_accounts = *matches.get_one::<u32>("accounts").unwrap();
    let requests_per_client = *matches.get_one::<usize>("requests").unwrap();
    let deposit_sleep = *matches.get_one::<u64>("deposit_sleep").unwrap();
    let transfer_sleep = *matches.get_one::<u64>("transfer_sleep").unwrap();
    let balance_sleep = *matches.get_one::<u64>("balance_sleep").unwrap();
    let client_sleep = *matches.get_one::<u64>("client_sleep").unwrap();

    // bank and accounts creation
    let mut accounts = HashMap::new();
    for id in 1..=num_accounts {
        accounts.insert(id, Account { balance: Mutex::new(1000.0) });
    }
    let bank = Arc::new(Bank {
        accounts,
        deposit_sleep: Duration::from_millis(deposit_sleep),
        transfer_sleep: Duration::from_millis(transfer_sleep),
        balance_sleep: Duration::from_millis(balance_sleep),
    });

    // request queue setup
    let request_queue = Arc::new(RequestQueue {
        queue: Mutex::new(Vec::new()),
        condvar: Condvar::new(),
        no_more_clients: AtomicBool::new(false),
    });

    let operation_count = Arc::new(AtomicUsize::new(0));

    // server thread
    let server_running = Arc::new(AtomicBool::new(true));
    let server_handle = {
        let request_queue = Arc::clone(&request_queue);
        let server_running = Arc::clone(&server_running);
        let operation_count = Arc::clone(&operation_count);
        thread::spawn(move || {
            let mut last_checked_count = 0;
            let mut empty_since = None;
            while server_running.load(Ordering::SeqCst) {
                let current_count = operation_count.load(Ordering::SeqCst);
                if current_count >= last_checked_count + 10 {
                    let mut queue = request_queue.queue.lock().unwrap();
                    queue.push(Request {
                        operation: Operation::GeneralBalance,
                    });
                    request_queue.condvar.notify_one();
                    last_checked_count += 10;
                }
                let queue = request_queue.queue.lock().unwrap();
                let is_empty = queue.is_empty();
                drop(queue);
                if is_empty && request_queue.no_more_clients.load(Ordering::SeqCst) {
                    if empty_since.is_none() {
                        empty_since = Some(Instant::now());
                    } else if empty_since.unwrap().elapsed() >= Duration::from_secs(5) {
                        println!("Finished processing all client requests.");
                        break;
                    }
                } else {
                    empty_since = None;
                }
                thread::sleep(Duration::from_millis(100));
            }
            server_running.store(false, Ordering::SeqCst);
        })
    };

    // worker threads
    let worker_running = Arc::new(AtomicBool::new(true));
    let mut worker_handles = Vec::new();
    for id in 0..num_workers {
        let bank = Arc::clone(&bank);
        let request_queue = Arc::clone(&request_queue);
        let worker_running = Arc::clone(&worker_running);
        let operation_count = Arc::clone(&operation_count);
        let handle = thread::spawn(move || {
            while worker_running.load(Ordering::SeqCst) {
                let mut queue = request_queue.queue.lock().unwrap();
                loop {
                    if let Some(request) = queue.pop() {
                        drop(queue);
                        println!("Worker {} is now executing.", id);
                        match &request.operation {
                            Operation::Deposit { account_id, amount } => {
                                thread::sleep(bank.deposit_sleep);
                                if let Some(account) = bank.accounts.get(account_id) {
                                    let mut balance = account.balance.lock().unwrap();
                                    *balance += amount;
                                    println!(
                                        "Deposit: Account {}: Amount {:.2}, New Balance: {:.2}",
                                        account_id, amount, *balance
                                    );
                                } else {
                                    eprintln!("Deposit failed: Account {} does not exist.", account_id);
                                }
                            }
                            Operation::Transfer { from_account_id, to_account_id, amount } => {
                                if from_account_id == to_account_id {
                                    eprintln!("Transfer failed: Cannot transfer to the same account.");
                                    return;
                                }
                                thread::sleep(bank.transfer_sleep);
                                let from_account = bank.accounts.get(from_account_id);
                                let to_account = bank.accounts.get(to_account_id);
                                if from_account.is_none() || to_account.is_none() {
                                    if from_account.is_none() {
                                        eprintln!("Transfer failed: Source Account {} does not exist.", from_account_id);
                                    }
                                    if to_account.is_none() {
                                        eprintln!("Transfer failed: Destination Account {} does not exist.", to_account_id);
                                    }
                                    return;
                                }
                                let (first_account, second_account) = if from_account_id < to_account_id {
                                    (from_account.unwrap(), to_account.unwrap())
                                } else {
                                    (to_account.unwrap(), from_account.unwrap())
                                };
                                let first_balance = first_account.balance.lock().unwrap();
                                let second_balance = second_account.balance.lock().unwrap();
                                let (mut from_balance, mut to_balance) = if from_account_id < to_account_id {
                                    (first_balance, second_balance)
                                } else {
                                    (second_balance, first_balance)
                                };
                                if *from_balance >= *amount {
                                    *from_balance -= *amount;
                                    *to_balance += *amount;
                                    println!(
                                        "Transfer: From Account {} to Account {}: Amount {:.2}",
                                        from_account_id, to_account_id, amount
                                    );
                                } else {
                                    eprintln!("Transfer failed: Account {} has insufficient funds.", from_account_id);
                                }
                            }
                            Operation::GeneralBalance => {
                                thread::sleep(bank.balance_sleep);
                                println!("\n--- General Balance Snapshot ---");
                                for (id, account) in &bank.accounts {
                                    let balance = account.balance.lock().unwrap();
                                    println!("Account {}: Balance {:.2}", id, *balance);
                                }
                                println!("--------------------------------\n");
                            }
                        }
                        operation_count.fetch_add(1, Ordering::SeqCst);
                        println!("Worker {} is now idle.", id);
                        break;
                    }
                    if request_queue.no_more_clients.load(Ordering::SeqCst) {
                        drop(queue);
                        return;
                    }
                    let (new_queue, _) = request_queue.condvar.wait_timeout(queue, Duration::from_secs(1)).unwrap();
                    queue = new_queue;
                    if request_queue.no_more_clients.load(Ordering::SeqCst) && queue.is_empty() {
                        drop(queue);
                        return;
                    }
                }
                thread::sleep(Duration::from_millis(100));
            }
            println!("Worker {} is terminating.", id);
        });
        worker_handles.push(handle);
    }

    // client threads
    let mut client_handles = Vec::new();
    for _ in 0..num_clients {
        let request_queue = Arc::clone(&request_queue);
        let handle = thread::spawn(move || {
            let mut rng = rand::thread_rng();
            for _ in 0..requests_per_client {
                let operation = if rng.gen_bool(0.5) {
                    Operation::Deposit {
                        account_id: rng.gen_range(1..=num_accounts),
                        amount: rng.gen_range(-100.0..100.0),
                    }
                } else {
                    let from_account_id = rng.gen_range(1..=num_accounts);
                    let mut to_account_id = rng.gen_range(1..=num_accounts);
                    while from_account_id == to_account_id {
                        to_account_id = rng.gen_range(1..=num_accounts);
                    }
                    Operation::Transfer {
                        from_account_id,
                        to_account_id,
                        amount: rng.gen_range(-100.0..100.0 as f64).abs(),
                    }
                };
                let mut queue = request_queue.queue.lock().unwrap();
                queue.push(Request { operation });
                request_queue.condvar.notify_one();
                drop(queue);
                thread::sleep(Duration::from_millis(client_sleep));
            }
        });
        client_handles.push(handle);
    }

    // graceful shutdown
    for handle in client_handles {
        handle.join().unwrap();
    }

    request_queue.no_more_clients.store(true, Ordering::SeqCst);
    server_handle.join().unwrap();
    worker_running.store(false, Ordering::SeqCst);

    for handle in worker_handles {
        handle.join().unwrap();
    }
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