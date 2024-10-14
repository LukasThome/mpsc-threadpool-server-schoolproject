use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Condvar, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};
use rand::Rng;
use clap::{Arg, Command};

fn main() {
    let matches = Command::new("Bank Server Simulation")
        .version("1.0")
        .author("Your Name")
        .about("Simulates a bank server with client requests and worker threads")
        .arg(
            Arg::new("workers")
                .long("workers")
                .value_name("NUM")
                .help("Number of worker threads")
                .default_value("5")
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("clients")
                .long("clients")
                .value_name("NUM")
                .help("Number of client threads")
                .default_value("10")
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("accounts")
                .long("accounts")
                .value_name("NUM")
                .help("Number of bank accounts")
                .default_value("100")
                .value_parser(clap::value_parser!(u32)),
        )
        .arg(
            Arg::new("requests")
                .long("requests")
                .value_name("NUM")
                .help("Number of requests per client")
                .default_value("100")
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("deposit_sleep")
                .long("deposit-sleep")
                .value_name("MILLIS")
                .help("Sleep duration for deposit operation in milliseconds")
                .default_value("100")
                .value_parser(clap::value_parser!(u64)),
        )
        .arg(
            Arg::new("transfer_sleep")
                .long("transfer-sleep")
                .value_name("MILLIS")
                .help("Sleep duration for transfer operation in milliseconds")
                .default_value("150")
                .value_parser(clap::value_parser!(u64)),
        )
        .arg(
            Arg::new("balance_sleep")
                .long("balance-sleep")
                .value_name("MILLIS")
                .help("Sleep duration for balance operation in milliseconds")
                .default_value("200")
                .value_parser(clap::value_parser!(u64)),
        )
        .arg(
            Arg::new("client_sleep")
                .long("client-sleep")
                .value_name("MILLIS")
                .help("Sleep duration between client requests in milliseconds")
                .default_value("50")
                .value_parser(clap::value_parser!(u64)),
        )
        .get_matches();

    let num_workers = *matches.get_one::<usize>("workers").unwrap();
    let num_clients = *matches.get_one::<usize>("clients").unwrap();
    let num_accounts = *matches.get_one::<u32>("accounts").unwrap();
    let requests_per_client = *matches.get_one::<usize>("requests").unwrap();
    let deposit_sleep = *matches.get_one::<u64>("deposit_sleep").unwrap();
    let transfer_sleep = *matches.get_one::<u64>("transfer_sleep").unwrap();
    let balance_sleep = *matches.get_one::<u64>("balance_sleep").unwrap();
    let client_sleep = *matches.get_one::<u64>("client_sleep").unwrap();

    let bank = Arc::new(Bank::new(
        num_accounts,
        deposit_sleep,
        transfer_sleep,
        balance_sleep,
    ));

    let request_queue = Arc::new(RequestQueue::new());

    let operation_count = Arc::new(AtomicUsize::new(0));

    let server_running = Arc::new(AtomicBool::new(true));
    let server_handle = {
        let request_queue = Arc::clone(&request_queue);
        let server_running = Arc::clone(&server_running);
        let operation_count = Arc::clone(&operation_count);
        thread::spawn(move || {
            Server::run(request_queue, operation_count, server_running);
        })
    };

    let worker_running = Arc::new(AtomicBool::new(true));
    let mut worker_handles = Vec::new();
    for id in 0..num_workers {
        let bank = Arc::clone(&bank);
        let request_queue = Arc::clone(&request_queue);
        let worker_running = Arc::clone(&worker_running);
        let operation_count = Arc::clone(&operation_count);
        let handle = thread::spawn(move || {
            Worker::run(id, bank, request_queue, worker_running, operation_count);
        });
        worker_handles.push(handle);
    }

    let mut client_handles = Vec::new();
    for _ in 0..num_clients {
        let request_queue = Arc::clone(&request_queue);
        let client = Client::new(
            request_queue,
            num_accounts,
            requests_per_client,
            client_sleep,
        );
        let handle = thread::spawn(move || {
            client.run();
        });
        client_handles.push(handle);
    }

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
    balance: Mutex<f64>
}

struct Bank {
    accounts: HashMap<u32, Account>,
    deposit_sleep: Duration,
    transfer_sleep: Duration,
    balance_sleep: Duration,
}

impl Bank {
    fn new(
        num_accounts: u32,
        deposit_sleep_ms: u64,
        transfer_sleep_ms: u64,
        balance_sleep_ms: u64,
    ) -> Self {
        let mut accounts = HashMap::new();
        for id in 1..=num_accounts {
            accounts.insert(
                id,
                Account {
                    balance: Mutex::new(1000.0),
                },
            );
        }
        Bank {
            accounts,
            deposit_sleep: Duration::from_millis(deposit_sleep_ms),
            transfer_sleep: Duration::from_millis(transfer_sleep_ms),
            balance_sleep: Duration::from_millis(balance_sleep_ms),
        }
    }

    fn deposit(&self, account_id: u32, amount: f64) {
        thread::sleep(self.deposit_sleep);

        if let Some(account) = self.accounts.get(&account_id) {
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

    fn transfer(&self, from_account_id: u32, to_account_id: u32, amount: f64) {
        if from_account_id == to_account_id {
            eprintln!("Transfer failed: Cannot transfer to the same account.");
            return;
        }

        thread::sleep(self.transfer_sleep);

        let accounts = &self.accounts;

        let from_account = accounts.get(&from_account_id);
        let to_account = accounts.get(&to_account_id);

        if from_account.is_none() || to_account.is_none() {
            if from_account.is_none() {
                eprintln!(
                    "Transfer failed: Source Account {} does not exist.",
                    from_account_id
                );
            }
            if to_account.is_none() {
                eprintln!(
                    "Transfer failed: Destination Account {} does not exist.",
                    to_account_id
                );
            }
            return;
        }

        let (first_id, first_account, second_account) =
            if from_account_id < to_account_id {
                (
                    from_account_id,
                    from_account.unwrap(),
                    to_account.unwrap(),
                )
            } else {
                (
                    to_account_id,
                    to_account.unwrap(),
                    from_account.unwrap(),
                )
            };

        let first_balance = first_account.balance.lock().unwrap();
        let second_balance = second_account.balance.lock().unwrap();

        let (mut from_balance, mut to_balance) = if from_account_id == first_id {
            (first_balance, second_balance)
        } else {
            (second_balance, first_balance)
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

    fn general_balance(&self) {
        thread::sleep(self.balance_sleep);

        println!("\n--- General Balance Snapshot ---");
        for (id, account) in &self.accounts {
            let balance = account.balance.lock().unwrap();
            println!("Account {}: Balance {:.2}", id, *balance);
        }
        println!("--------------------------------\n");
    }
}

#[derive(Clone)]
enum Operation {
    Deposit { account_id: u32, amount: f64 },
    Transfer {
        from_account_id: u32,
        to_account_id: u32,
        amount: f64,
    },
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

impl RequestQueue {
    fn new() -> Self {
        RequestQueue {
            queue: Mutex::new(Vec::new()),
            condvar: Condvar::new(),
            no_more_clients: AtomicBool::new(false),
        }
    }

    fn push(&self, request: Request) {
        let mut queue = self.queue.lock().unwrap();
        queue.push(request);
        self.condvar.notify_one();
    }

    fn pop(&self) -> Option<Request> {
        let mut queue = self.queue.lock().unwrap();
        loop {
            if let Some(request) = queue.pop() {
                return Some(request);
            }
            if self.no_more_clients.load(Ordering::SeqCst) {
                return None;
            }
            let (new_queue, _) = self.condvar.wait_timeout(queue, Duration::from_secs(1)).unwrap();
            queue = new_queue;
            if self.no_more_clients.load(Ordering::SeqCst) && queue.is_empty() {
                return None;
            }            
        }
    }

    fn is_empty(&self) -> bool {
        let queue = self.queue.lock().unwrap();
        queue.is_empty()
    }
}

struct Worker;

impl Worker {
    fn run(
        id: usize,
        bank: Arc<Bank>,
        request_queue: Arc<RequestQueue>,
        running: Arc<AtomicBool>,
        operation_count: Arc<AtomicUsize>,
    ) {
        while running.load(Ordering::SeqCst) {
            if let Some(request) = request_queue.pop() {
                println!("Worker {} is now executing.", id);
                Worker::execute_operation(&bank, &request.operation);

                match request.operation {
                    Operation::Deposit { .. } | Operation::Transfer { .. } => {
                        operation_count.fetch_add(1, Ordering::SeqCst);
                    }
                    _ => {}
                }

                println!("Worker {} is now idle.", id);
            } else {
                thread::sleep(Duration::from_millis(100));
            }
        }
        println!("Worker {} is terminating.", id);
    }

    fn execute_operation(bank: &Bank, operation: &Operation) {
        match operation {
            Operation::Deposit { account_id, amount } => {
                bank.deposit(*account_id, *amount);
            }
            Operation::Transfer {
                from_account_id,
                to_account_id,
                amount,
            } => {
                bank.transfer(*from_account_id, *to_account_id, *amount);
            }
            Operation::GeneralBalance => {
                bank.general_balance();
            }
        }
    }
}

struct Server;

impl Server {
    fn run(
        request_queue: Arc<RequestQueue>,
        operation_count: Arc<AtomicUsize>,
        running: Arc<AtomicBool>,
    ) {
        let mut last_checked_count = 0;
        let mut empty_since = None;

        while running.load(Ordering::SeqCst) {
            let current_count = operation_count.load(Ordering::SeqCst);

            if current_count >= last_checked_count + 10 {
                let general_balance_request = Request {
                    operation: Operation::GeneralBalance,
                };
                request_queue.push(general_balance_request);
                last_checked_count += 10;
            }

            if request_queue.is_empty() && request_queue.no_more_clients.load(Ordering::SeqCst) {
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

        running.store(false, Ordering::SeqCst);
    }
}

struct Client {
    request_queue: Arc<RequestQueue>,
    num_accounts: u32,
    requests_per_client: usize,
    client_sleep: Duration,
}

impl Client {
    fn new(
        request_queue: Arc<RequestQueue>,
        num_accounts: u32,
        requests_per_client: usize,
        client_sleep_ms: u64,
    ) -> Self {
        Client {
            request_queue,
            num_accounts,
            requests_per_client,
            client_sleep: Duration::from_millis(client_sleep_ms),
        }
    }

    fn run(&self) {
        let mut rng = rand::thread_rng();
        for _ in 0..self.requests_per_client {
            let operation = if rng.gen_bool(0.5) {
                let account_id = self.random_account_id(&mut rng);
                let amount = self.random_amount(&mut rng);
                Operation::Deposit { account_id, amount }
            } else {
                let from_account_id = self.random_account_id(&mut rng);
                let mut to_account_id = self.random_account_id(&mut rng);
                while from_account_id == to_account_id {
                    to_account_id = self.random_account_id(&mut rng);
                }
                let amount = self.random_amount(&mut rng).abs();
                Operation::Transfer {
                    from_account_id,
                    to_account_id,
                    amount,
                }
            };

            let request = Request { operation };
            self.request_queue.push(request);

            thread::sleep(self.client_sleep);
        }
    }

    fn random_account_id(&self, rng: &mut impl Rng) -> u32 {
        rng.gen_range(1..=self.num_accounts)
    }

    fn random_amount(&self, rng: &mut impl Rng) -> f64 {
        rng.gen_range(-100.0..100.0)
    }
}
