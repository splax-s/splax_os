# Splax OS Tutorial Series

Welcome to the Splax OS tutorial series! These tutorials will guide you from beginner to advanced usage.

## Tutorial Index

1. [Hello World](#tutorial-1-hello-world)
2. [File Operations](#tutorial-2-file-operations)
3. [Capability Basics](#tutorial-3-capability-basics)
4. [Writing Your First Service](#tutorial-4-writing-your-first-service)
5. [IPC Communication](#tutorial-5-ipc-communication)
6. [Network Programming](#tutorial-6-network-programming)
7. [Container Deployment](#tutorial-7-container-deployment)
8. [Building a Full Application](#tutorial-8-building-a-full-application)

---

## Tutorial 1: Hello World

**Objective**: Write and run your first Splax OS program.

### Step 1: Create the Source File

```bash
splax> edit hello.rs
```

Add this code:

```rust
fn main() {
    println!("Hello, Splax OS!");
}
```

### Step 2: Compile to WASM

```bash
# Install the Rust WASM toolchain
splax> pkg install rust-wasm

# Compile
splax> rustc hello.rs --target wasm32-wasi -o hello.wasm
```

### Step 3: Run the Program

```bash
splax> run hello.wasm
Hello, Splax OS!
```

### What You Learned

- Creating source files in Splax OS
- Compiling Rust to WebAssembly
- Running WASM programs

---

## Tutorial 2: File Operations

**Objective**: Learn to work with files and directories.

### Creating Files and Directories

```bash
# Create a directory
splax> mkdir projects

# Navigate into it
splax> cd projects

# Create a file
splax> touch notes.txt

# Write content
splax> echo "My first note" > notes.txt

# View content
splax> cat notes.txt
My first note
```

### File Manipulation

```bash
# Copy a file
splax> cp notes.txt backup.txt

# Rename/move
splax> mv backup.txt archive/notes-backup.txt

# List files
splax> ls -l
total 1
-rw-r--r-- 1 user 14 Jan  4 10:00 notes.txt

# Remove file
splax> rm notes.txt
```

### Working with Capabilities

Files in Splax OS are protected by capabilities:

```bash
# View file capabilities
splax> cap show notes.txt

# Grant read access to another process
splax> cap grant notes.txt --read --to process:1234
```

### What You Learned

- Basic file and directory operations
- How capabilities protect file access
- Moving and copying files

---

## Tutorial 3: Capability Basics

**Objective**: Understand Splax OS's capability-based security.

### Understanding Capabilities

A capability is an unforgeable token that grants specific access rights:

```
┌─────────────────────────────────────┐
│           Capability Token          │
├─────────────────────────────────────┤
│  Resource: file:/home/user/doc.txt  │
│  Permissions: read, write           │
│  Owner: process:1234                │
│  Expiry: 2026-01-05 10:00:00        │
│  Signature: [cryptographic sig]     │
└─────────────────────────────────────┘
```

### Viewing Your Capabilities

```bash
splax> cap list
ID      RESOURCE                PERMISSIONS     EXPIRES
1       file:/home/*            read,write      never
2       net:*:80                connect         never
3       service:s-atlas         call            never
```

### Creating Capabilities

```bash
# Create a capability for a file
splax> cap create file:/data/shared.txt --permissions read,write
Created capability: cap-7f8a2b3c

# Create a time-limited capability
splax> cap create net:8080 --permissions listen --expires 1h
Created capability: cap-9d4e5f6g (expires in 1 hour)
```

### Delegating Capabilities

Share access with other processes:

```bash
# Delegate to a process
splax> cap delegate cap-7f8a2b3c --to process:5678

# Delegate with reduced permissions (attenuation)
splax> cap delegate cap-7f8a2b3c --to process:5678 --only read
```

### Revoking Capabilities

```bash
# Revoke a specific capability
splax> cap revoke cap-7f8a2b3c

# Revoke all capabilities for a resource
splax> cap revoke --resource file:/data/shared.txt
```

### What You Learned

- How capabilities work
- Creating and viewing capabilities
- Delegation and revocation

---

## Tutorial 4: Writing Your First Service

**Objective**: Create a simple service that responds to IPC messages.

### Step 1: Create the Service

Create `echo_service.rs`:

```rust
use splax::service::{Service, ServiceBuilder};
use splax::ipc::{Message, Response};

struct EchoService;

impl Service for EchoService {
    fn name(&self) -> &str {
        "echo"
    }

    fn handle(&self, msg: Message) -> Response {
        // Echo back the message
        Response::ok(msg.payload())
    }
}

fn main() {
    let service = ServiceBuilder::new()
        .name("echo")
        .version("1.0.0")
        .handler(EchoService)
        .build();

    service.run();
}
```

### Step 2: Compile and Install

```bash
# Compile
splax> rustc echo_service.rs --target wasm32-wasi -o echo.wasm

# Install as service
splax> svc install echo.wasm --name echo-service
```

### Step 3: Start the Service

```bash
# Start
splax> svc start echo-service

# Check status
splax> svc status echo-service
echo-service: running (pid: 1234)
  uptime: 5s
  memory: 2.1 MB
```

### Step 4: Test the Service

```bash
# Send a message
splax> ipc call echo-service "Hello!"
Response: "Hello!"
```

### What You Learned

- Creating a service in Rust
- Installing and running services
- Communicating with services via IPC

---

## Tutorial 5: IPC Communication

**Objective**: Master inter-process communication patterns.

### Request-Response Pattern

```rust
use splax::ipc::{Channel, Request};

fn main() {
    // Connect to a service
    let channel = Channel::connect("calculator-service").unwrap();

    // Send request
    let request = Request::new("add", &[1, 2]);
    let response = channel.call(request).unwrap();

    println!("Result: {}", response.as_i32());
}
```

### Pub-Sub Pattern

Publisher:
```rust
use splax::ipc::Topic;

fn main() {
    let topic = Topic::create("events").unwrap();

    loop {
        topic.publish("New event occurred");
        sleep(1000);
    }
}
```

Subscriber:
```rust
use splax::ipc::Topic;

fn main() {
    let topic = Topic::subscribe("events").unwrap();

    for event in topic.iter() {
        println!("Received: {}", event);
    }
}
```

### Streaming Pattern

```rust
use splax::ipc::{Stream, StreamItem};

fn main() {
    let stream = Stream::open("data-stream").unwrap();

    // Read items as they arrive
    while let Some(item) = stream.next() {
        process(item);
    }
}
```

### Zero-Copy Transfer

For large data:
```rust
use splax::ipc::SharedBuffer;

fn main() {
    // Create shared buffer
    let buffer = SharedBuffer::new(1024 * 1024);
    buffer.write(large_data);

    // Transfer ownership (no copy!)
    channel.send_buffer(buffer);
}
```

### What You Learned

- Different IPC patterns
- Request-response, pub-sub, streaming
- Zero-copy for performance

---

## Tutorial 6: Network Programming

**Objective**: Build networked applications.

### TCP Client

```rust
use splax::net::{TcpStream, SocketAddr};

fn main() {
    let addr: SocketAddr = "93.184.216.34:80".parse().unwrap();
    let mut stream = TcpStream::connect(addr).unwrap();

    // Send HTTP request
    stream.write_all(b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n").unwrap();

    // Read response
    let mut buffer = [0; 4096];
    let n = stream.read(&mut buffer).unwrap();
    println!("{}", String::from_utf8_lossy(&buffer[..n]));
}
```

### TCP Server

```rust
use splax::net::{TcpListener, TcpStream};

fn main() {
    let listener = TcpListener::bind("0.0.0.0:8080").unwrap();
    println!("Listening on port 8080");

    for stream in listener.incoming() {
        handle_client(stream.unwrap());
    }
}

fn handle_client(mut stream: TcpStream) {
    let mut buffer = [0; 1024];
    stream.read(&mut buffer).unwrap();
    stream.write_all(b"HTTP/1.1 200 OK\r\n\r\nHello!").unwrap();
}
```

### UDP Communication

```rust
use splax::net::UdpSocket;

fn main() {
    let socket = UdpSocket::bind("0.0.0.0:9000").unwrap();

    let mut buffer = [0; 1024];
    let (n, src) = socket.recv_from(&mut buffer).unwrap();

    println!("Received from {}: {}", src, String::from_utf8_lossy(&buffer[..n]));

    socket.send_to(b"ACK", src).unwrap();
}
```

### Using the HTTP Client

```bash
# Simple GET request
splax> http get https://api.example.com/data

# POST with JSON
splax> http post https://api.example.com/users --json '{"name": "Alice"}'

# With headers
splax> http get https://api.example.com --header "Authorization: Bearer token"
```

### What You Learned

- TCP client and server programming
- UDP communication
- HTTP client usage

---

## Tutorial 7: Container Deployment

**Objective**: Deploy applications using S-CLUSTER.

### Creating a Deployment

```yaml
# deployment.yaml
apiVersion: splax/v1
kind: Deployment
metadata:
  name: web-app
spec:
  replicas: 3
  selector:
    app: web
  template:
    spec:
      containers:
      - name: web
        image: my-web-app.wasm
        ports:
        - containerPort: 8080
        resources:
          limits:
            memory: 128Mi
            cpu: 100m
```

### Deploying

```bash
# Apply the deployment
splax> cluster apply deployment.yaml

# Check status
splax> cluster get deployments
NAME      READY   UP-TO-DATE   AVAILABLE
web-app   3/3     3            3

# View pods
splax> cluster get pods
NAME            STATUS    RESTARTS   AGE
web-app-abc12   Running   0          1m
web-app-def34   Running   0          1m
web-app-ghi56   Running   0          1m
```

### Creating a Service

```yaml
# service.yaml
apiVersion: splax/v1
kind: Service
metadata:
  name: web-service
spec:
  type: ClusterIP
  selector:
    app: web
  ports:
  - port: 80
    targetPort: 8080
```

```bash
splax> cluster apply service.yaml

# Access via internal DNS
splax> http get http://web-service.default.svc:80
```

### Scaling

```bash
# Scale up
splax> cluster scale deployment/web-app --replicas=5

# Auto-scaling
splax> cluster autoscale deployment/web-app --min=2 --max=10 --cpu-percent=50
```

### Rolling Updates

```bash
# Update image
splax> cluster set deployment/web-app image=my-web-app-v2.wasm

# Watch rollout
splax> cluster rollout status deployment/web-app
deployment "web-app" successfully rolled out
```

### What You Learned

- Deploying containerized apps
- Service networking
- Scaling and rolling updates

---

## Tutorial 8: Building a Full Application

**Objective**: Build a complete todo list application.

### Architecture

```
┌─────────────────────────────────────────┐
│              CLI Client                  │
└─────────────────┬───────────────────────┘
                  │ IPC
┌─────────────────▼───────────────────────┐
│           Todo Service                   │
│  (business logic, validation)            │
└─────────────────┬───────────────────────┘
                  │ IPC
┌─────────────────▼───────────────────────┐
│          Storage Service                 │
│  (persistence, queries)                  │
└─────────────────────────────────────────┘
```

### Step 1: Data Types

Create `types.rs`:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: u64,
    pub title: String,
    pub completed: bool,
    pub created_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TodoRequest {
    List,
    Add(String),
    Complete(u64),
    Delete(u64),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TodoResponse {
    List(Vec<Todo>),
    Added(Todo),
    Completed(Todo),
    Deleted(u64),
    Error(String),
}
```

### Step 2: Todo Service

Create `todo_service.rs`:
```rust
use splax::service::{Service, ServiceBuilder};
use splax::ipc::{Message, Response};
use splax::storage::ObjectStore;

struct TodoService {
    store: ObjectStore,
    next_id: u64,
}

impl TodoService {
    fn new() -> Self {
        Self {
            store: ObjectStore::open("todos").unwrap(),
            next_id: 1,
        }
    }

    fn list(&self) -> Vec<Todo> {
        self.store.list_all().unwrap()
    }

    fn add(&mut self, title: String) -> Todo {
        let todo = Todo {
            id: self.next_id,
            title,
            completed: false,
            created_at: time::now(),
        };
        self.next_id += 1;
        self.store.put(&todo.id.to_string(), &todo).unwrap();
        todo
    }

    fn complete(&mut self, id: u64) -> Option<Todo> {
        if let Ok(mut todo) = self.store.get::<Todo>(&id.to_string()) {
            todo.completed = true;
            self.store.put(&id.to_string(), &todo).unwrap();
            Some(todo)
        } else {
            None
        }
    }

    fn delete(&mut self, id: u64) -> bool {
        self.store.delete(&id.to_string()).is_ok()
    }
}

impl Service for TodoService {
    fn name(&self) -> &str {
        "todo-service"
    }

    fn handle(&mut self, msg: Message) -> Response {
        let request: TodoRequest = msg.deserialize().unwrap();
        
        let response = match request {
            TodoRequest::List => TodoResponse::List(self.list()),
            TodoRequest::Add(title) => TodoResponse::Added(self.add(title)),
            TodoRequest::Complete(id) => match self.complete(id) {
                Some(todo) => TodoResponse::Completed(todo),
                None => TodoResponse::Error("Not found".into()),
            },
            TodoRequest::Delete(id) => {
                if self.delete(id) {
                    TodoResponse::Deleted(id)
                } else {
                    TodoResponse::Error("Not found".into())
                }
            }
        };
        
        Response::json(&response)
    }
}

fn main() {
    ServiceBuilder::new()
        .name("todo-service")
        .version("1.0.0")
        .handler(TodoService::new())
        .build()
        .run();
}
```

### Step 3: CLI Client

Create `todo_cli.rs`:
```rust
use splax::ipc::Channel;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    let channel = Channel::connect("todo-service").unwrap();

    let request = match args.get(1).map(|s| s.as_str()) {
        Some("list") | None => TodoRequest::List,
        Some("add") => {
            let title = args[2..].join(" ");
            TodoRequest::Add(title)
        }
        Some("done") => {
            let id: u64 = args[2].parse().unwrap();
            TodoRequest::Complete(id)
        }
        Some("rm") => {
            let id: u64 = args[2].parse().unwrap();
            TodoRequest::Delete(id)
        }
        _ => {
            println!("Usage: todo [list|add <title>|done <id>|rm <id>]");
            return;
        }
    };

    let response: TodoResponse = channel.call(&request).unwrap();
    
    match response {
        TodoResponse::List(todos) => {
            for todo in todos {
                let check = if todo.completed { "✓" } else { " " };
                println!("[{}] {} - {}", check, todo.id, todo.title);
            }
        }
        TodoResponse::Added(todo) => {
            println!("Added: {} - {}", todo.id, todo.title);
        }
        TodoResponse::Completed(todo) => {
            println!("Completed: {}", todo.title);
        }
        TodoResponse::Deleted(id) => {
            println!("Deleted todo #{}", id);
        }
        TodoResponse::Error(e) => {
            println!("Error: {}", e);
        }
    }
}
```

### Step 4: Deploy and Use

```bash
# Compile
splax> rustc todo_service.rs --target wasm32-wasi -o todo-svc.wasm
splax> rustc todo_cli.rs --target wasm32-wasi -o todo.wasm

# Install service
splax> svc install todo-svc.wasm --name todo-service
splax> svc start todo-service

# Use the CLI
splax> run todo.wasm add "Learn Splax OS"
Added: 1 - Learn Splax OS

splax> run todo.wasm add "Build an app"
Added: 2 - Build an app

splax> run todo.wasm list
[ ] 1 - Learn Splax OS
[ ] 2 - Build an app

splax> run todo.wasm done 1
Completed: Learn Splax OS

splax> run todo.wasm list
[✓] 1 - Learn Splax OS
[ ] 2 - Build an app
```

### What You Learned

- Building multi-component applications
- Service-oriented architecture
- Data persistence with S-STORAGE
- CLI application development

---

## Next Steps

Congratulations! You've completed the Splax OS tutorial series. Here's what to explore next:

1. **Read the API Documentation**: `./docs/API.md`
2. **Explore the Architecture**: `./docs/ARCHITECTURE.md`
3. **Join the Community**: https://community.splax-os.org
4. **Contribute**: `./CONTRIBUTING.md`

---

*Tutorial Series Version: 1.0*
*Last Updated: January 2026*
