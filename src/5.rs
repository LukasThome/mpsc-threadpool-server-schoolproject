use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};
use rand::{Rng, prelude::IteratorRandom};
use clap::{Arg, Command};
use std::ptr;

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

    let (
        num_workers,
        num_clients,
        num_accounts,
        requests_per_client,
        deposit_sleep,
        transfer_sleep,
        balance_sleep,
        client_sleep,
    ) = (
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
        accounts: (1..=num_accounts)
            .map(|id| (id, Account { balance: std::sync::Mutex::new(1000.0) }))
            .collect(),
        deposit_sleep,
        transfer_sleep,
        balance_sleep,
    });

    let request_queue = Arc::new(LockFreeQueue::new());
    let no_more_clients = Arc::new(AtomicBool::new(false));

    let operation_count = Arc::new(AtomicUsize::new(0));
    let server_running = Arc::new(AtomicBool::new(true));

    // server thread
    let server_handle = {
        let request_queue = Arc::clone(&request_queue);
        let server_running = Arc::clone(&server_running);
        let operation_count = Arc::clone(&operation_count);
        let no_more_clients = Arc::clone(&no_more_clients);
        thread::spawn(move || {
            let (mut last_checked_count, mut empty_since) = (0, None);
            while server_running.load(Ordering::SeqCst) {
                if operation_count.load(Ordering::SeqCst) >= last_checked_count + 10 {
                    request_queue.enqueue(Operation::GeneralBalance);
                    last_checked_count += 10;
                }
                if request_queue.is_empty() && no_more_clients.load(Ordering::SeqCst) {
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
    let workers_running = Arc::new(AtomicBool::new(true));
    let worker_handles: Vec<_> = (0..num_workers)
        .map(|id| {
            let bank = Arc::clone(&bank);
            let request_queue = Arc::clone(&request_queue);
            let workers_running = Arc::clone(&workers_running);
            let operation_count = Arc::clone(&operation_count);
            let no_more_clients = Arc::clone(&no_more_clients);
            thread::spawn(move || {
                while workers_running.load(Ordering::SeqCst) {
                    if let Some(operation) = request_queue.dequeue() {
                        println!("Worker {} is now executing.", id);
                        match operation {
                            Operation::Deposit { account_id, amount } => {
                                thread::sleep(bank.deposit_sleep);
                                if let Some(account) = bank.accounts.get(&account_id) {
                                    *account.balance.lock().unwrap() += amount;
                                    println!("Deposit: Account {}: Amount {:.2}", account_id, amount);
                                }
                            }
                            Operation::Transfer { from_account_id, to_account_id, amount } => {
                                thread::sleep(bank.transfer_sleep);
                                if let (Some(from_account), Some(to_account)) = (
                                    bank.accounts.get(&from_account_id),
                                    bank.accounts.get(&to_account_id),
                                ) {
                                    let (mut from_balance, mut to_balance) = if from_account_id < to_account_id {
                                        (
                                            from_account.balance.lock().unwrap(),
                                            to_account.balance.lock().unwrap(),
                                        )
                                    } else {
                                        (
                                            to_account.balance.lock().unwrap(),
                                            from_account.balance.lock().unwrap(),
                                        )
                                    };
                                    if *from_balance >= amount {
                                        *from_balance -= amount;
                                        *to_balance += amount;
                                        println!(
                                            "Transfer: From Account {} to Account {}: Amount {:.2}",
                                            from_account_id, to_account_id, amount
                                        );
                                    } else {
                                        eprintln!(
                                            "Transfer failed: Account {} has insufficient funds.",
                                            from_account_id
                                        );
                                    }
                                }
                            }
                            Operation::GeneralBalance => {
                                thread::sleep(bank.balance_sleep);
                                println!("\n--- General Balance Snapshot ---");
                                bank.accounts.iter().for_each(|(id, account)| {
                                    println!("Account {}: Balance {:.2}", id, *account.balance.lock().unwrap())
                                });
                                println!("--------------------------------\n");
                            }
                        }
                        operation_count.fetch_add(1, Ordering::SeqCst);
                        println!("Worker {} is now idle.", id);
                    } else if no_more_clients.load(Ordering::SeqCst) && request_queue.is_empty() {
                        break;
                    } else {
                        thread::sleep(Duration::from_millis(100));
                    }
                }
                println!("Worker {} is terminating.", id);
            })
        })
        .collect();

    // client threads
    let client_handles: Vec<_> = (0..num_clients)
        .map(|_| {
            let request_queue = Arc::clone(&request_queue);
            thread::spawn(move || {
                let mut rng = rand::thread_rng();
                for _ in 0..requests_per_client {
                    let operation = if rng.gen_bool(0.5) {
                        Operation::Deposit {
                            account_id: rng.gen_range(1..=num_accounts),
                            amount: rng.gen_range(-100.0..100.0),
                        }
                    } else {
                        let from_account_id = rng.gen_range(1..=num_accounts);
                        let to_account_id = (1..=num_accounts)
                            .filter(|&id| id != from_account_id)
                            .choose(&mut rng)
                            .unwrap();
                        Operation::Transfer {
                            from_account_id,
                            to_account_id,
                            amount: rng.gen_range(0.0..100.0),
                        }
                    };
                    request_queue.enqueue(operation);
                    thread::sleep(client_sleep);
                }
            })
        })
        .collect();

    // graceful shutdown
    for handle in client_handles {
        handle.join().unwrap();
    }
    no_more_clients.store(true, Ordering::SeqCst);
    server_handle.join().unwrap();
    workers_running.store(false, Ordering::SeqCst);
    for handle in worker_handles {
        handle.join().unwrap();
    }

    println!(
        "Total client requests processed: {}",
        num_clients * requests_per_client
    );
}

struct Account {
    balance: std::sync::Mutex<f64>,
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

// Lock-free queue implementation
struct LockFreeQueue<T> {
    head: AtomicPtr<Node<T>>,
    tail: AtomicPtr<Node<T>>,
}

struct Node<T> {
    value: Option<T>,
    next: AtomicPtr<Node<T>>,
}

impl<T> LockFreeQueue<T> {
    fn new() -> Self {
        let dummy = Box::into_raw(Box::new(Node {
            value: None,
            next: AtomicPtr::new(ptr::null_mut()),
        }));
        Self {
            head: AtomicPtr::new(dummy),
            tail: AtomicPtr::new(dummy),
        }
    }

    fn enqueue(&self, value: T) {
        let new_node = Box::into_raw(Box::new(Node {
            value: Some(value),
            next: AtomicPtr::new(ptr::null_mut()),
        }));

        loop {
            let tail = self.tail.load(Ordering::Acquire);
            let next = unsafe { (*tail).next.load(Ordering::Acquire) };

            if next.is_null() {
                if unsafe { (*tail).next.compare_exchange(ptr::null_mut(), new_node, Ordering::SeqCst, Ordering::SeqCst).is_ok() } {
                    self.tail.compare_exchange(tail, new_node, Ordering::SeqCst, Ordering::SeqCst).ok();
                    break;
                }
            } else {
                self.tail.compare_exchange(tail, next, Ordering::SeqCst, Ordering::SeqCst).ok();
            }
        }
    }

    fn dequeue(&self) -> Option<T> {
        loop {
            let head = self.head.load(Ordering::Acquire);
            let tail = self.tail.load(Ordering::Acquire);
            let next = unsafe { (*head).next.load(Ordering::Acquire) };

            if head == tail {
                if next.is_null() {
                    return None; // Queue is empty
                }
                self.tail.compare_exchange(tail, next, Ordering::SeqCst, Ordering::SeqCst).ok();
            } else {
                if let Some(value) = unsafe { (*next).value.take() } {
                    if self.head.compare_exchange(head, next, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                        unsafe {
                            let _ = Box::from_raw(head); // Free the old dummy node
                        }
                        return Some(value);
                    }
                }
            }
        }
    }

    fn is_empty(&self) -> bool {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        let next = unsafe { (*head).next.load(Ordering::Acquire) };
        head == tail && next.is_null()
    }
}

impl<T> Drop for LockFreeQueue<T> {
    fn drop(&mut self) {
        while let Some(_) = self.dequeue() {}
        let dummy = self.head.load(Ordering::Relaxed);
        unsafe {
            let _ = Box::from_raw(dummy);
        }
    }
}
