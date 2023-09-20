# WebSocket Simple Version Control tool

Challenge deployment tool for Ret2Shell platform.

> [!WARNING]
> 
> The design purpose of this library and tool is to make it more convenient to upload and manage CTF challenges. However, its version control functionality is not well-designed and lacks sufficient test cases to ensure reliability. Therefore, it is **NOT RECOMMENDED** to use it for actual project management! Git and other version control tools that have undergone long-term development are always a **BETTER** choice!
> 
> 这个库与工具的设计目的是为了更方便的上传与管理 CTF 题目，其版本控制功能从设计上来说并不完善，同时也没有足够的测试用例来保证可靠性，**请不要在实际的项目管理中使用**！Git 与其他经历了长时间发展的版本控制工具始终是**更好**的选择！

## Usage

### Build

```shell
cargo build
```

the binary cli could be found at `target/release/wsvc`.

### Init repo

you can use `wsvc new <repo name>` to init a new repository. if you already have a project, you can use `wsvc init` inside the project directory to init a new repository.

### Config

before use it, you maybe need to configure some basic actions, such as author name and checkout default action.

```shell
wsvc config set commit.author [Author] --global # set author name
wsvc config set commit.auto_record [true/false] --global # set default checkout action
wsvc config set auth.account [account] --global # set account of origin, not affect if you use local server
wsvc config set auth.passwd [passwd] --global # set password of origin, not affect if you use local server
```

if `commit.auto_record` is enabled, `wsvc checkout` will automatically commit a record if the workspace is dirty.

### Commit a record

wsvc does not have stage area or other cache designs, `wsvc commit` is more likely to take a snapshot of the current project. you can use `wsvc commit` to commit a record directly.

```shell
wsvc commit -m "commit message" [-a author]
```

### List records

you can use `wsvc logs` to list all records. the `skip` and `limit` options are used to control the number of records displayed.

by default, `skip = 0` and `limit = 10`, these options are not necessary.

```shell
wsvc logs --skip 0 --limit 10
```

### Checkout record

if you want to checkout to some record, you can use `wsvc checkout [hash prefix]` to do it.

```shell
wsvc checkout 1234567
```

if you want to checkout to the latest record, you can use `wsvc checkout` without any arguments.

```shell
wsvc checkout
```

noticed that `wsvc` could accept any length of hex strings, if there are multiple records with the same prefix, `wsvc` will report an error and list all possible records.
