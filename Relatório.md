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


1. **Aumento do Número de Contas**:
   - Com mais contas, o uso de `Mutex` se intensifica, aumentando o tempo de bloqueio para leitura e escrita.
   - Em cenários com um número muito alto de contas, a frequência de operações de balanceamento geral deve ser monitorada, pois cada operação realiza uma travada em cada conta individualmente, o que pode resultar em aumento do tempo de resposta.

2. **Variação no Número de Threads (Clientes e Workers)**:
   - **Mais Clientes**: Aumenta a taxa de geração de operações, potencialmente sobrecarregando a fila de requisições. Um número excessivo de clientes pode causar atrasos, uma vez que os `workers` podem não conseguir processar as operações na mesma taxa de chegada.
   - **Mais Workers**: Com mais `workers`, o sistema consegue processar as operações mais rapidamente, mas aumenta o risco de contenção no acesso à fila de requisições e às contas. Recomenda-se balancear o número de `workers` de acordo com a quantidade de operações esperada.

3. **Intervalo de `Timeout` para Operações**:
   - Reduzir o tempo de `timeout` pode melhorar a responsividade ao liberar rapidamente recursos bloqueados, mas também pode aumentar a chance de falhas de operação em sistemas com carga alta.
   - Um `timeout` mais longo pode ajudar a garantir que operações complexas sejam completadas, mas também pode resultar em bloqueios mais longos em recursos compartilhados, afetando a performance.

4. **Frequência de Balanceamento Geral**:
   - Ajustar a frequência do balanço geral (a cada 10 operações, por exemplo) pode impactar o desempenho do servidor. Em sistemas com alta carga de transações, diminuir a frequência ajuda a reduzir o impacto no processamento.
   - Em sistemas com baixa frequência de requisições, aumentar a frequência de balanço geral pode fornecer feedback mais imediato do estado do banco, útil para auditoria e monitoramento.

5. **Tamanho da Fila de Requisições**:
   - Em cenários de alta concorrência, uma fila muito pequena pode ser insuficiente para armazenar todas as requisições, gerando congestionamento e atrasos no atendimento das operações.
   - Com uma fila maior, o sistema tem maior capacidade de absorver picos de requisições, mas aumenta o uso de memória e a complexidade para sincronizar as operações.
