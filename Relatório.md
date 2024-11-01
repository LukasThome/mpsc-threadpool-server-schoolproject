## Principais Decisões de Implementação

### Estrutura de Contas
Utilização de `HashMap<u32, Account>` com saldo protegido por `Mutex<f64>`.

### Operações e Sincronização
- **Depósito**: Aceita valores positivos e negativos para operações de depósito e saque, protegida por mutex.
- **Transferência**: Implementada com dois mutexes, bloqueando as contas envolvidas na ordem dos IDs para evitar deadlock.
- **Balanço Geral**: Adicionado periodicamente após 10 requisições, forçando uma captura do estado atual das contas.

### Modelo Produtor-Consumidor e Pool de Threads
Modelo produtor-consumidor, com um pool de threads fixo. Threads de clientes geram requisições e threads trabalhadoras consomem da fila gerenciada pelo servidor, usando `Condvar` para a sincronização.

### Fila de Requisições e Fila Interna
O sistema utiliza duas filas:

- **Fila de Requisições**: Gerenciada pelo servidor e utilizada pelas threads de clientes para enviar operações. O servidor monitora essa fila, consumindo as requisições e repassando-as para a fila interna.
  
- **Fila Interna**: Utilizada pelas threads do pool de trabalhadores. O servidor adiciona operações consumidas da fila de requisições e insere também periodicamente a operação de balanço geral.

Ambas as filas utilizam `Mutex` para proteção e a fila interna usa `Condvar` para notificação das threads trabalhadoras.

### Controle de Concorrência
Uso de `Mutex` para proteger as contas e as filas de requisições e interna, além de `AtomicBool` para controle de estados globais.

### Simulação de Delays
Adição de `thread::sleep` nas operações para simular tempo de execução e demonstrar o comportamento concorrente.

## Instruções de Execução
Para compilar o código, é necessário ter o Rust instalado. Caso ainda não tenha o Rust instalado, siga as instruções disponíveis no site oficial [rust-lang.org](https://www.rust-lang.org/tools/install) (recomendado) ou tente `apt-get install rust`. Uma vez instalado, navegue até o diretório onde o arquivo de código está localizado e execute o seguinte comando:

```bash
cargo run --release -- --help
```

## Observações Conforme Parametrização Variada
TODO
