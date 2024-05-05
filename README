# BackupPC Pool Reader

BackupPC Pool Reader is a simple tool designed to read the BackupPC pool and list the files in it. This tool is built
using Rust, providing efficient and fast performance.

## Features

- Read the BackupPC pool: The tool can access and read the BackupPC pool, providing a list of files within it.
- Fast and efficient: Built with Rust, this tool provides fast and efficient performance.

## Installation

Ensure you have Rust and Cargo installed on your machine. If not, you can install them from [here](https://www.rust-lang.org/tools/install).

For using the library in your project:

```bash
cargo add backuppc_pool_reader
```

For using the tool:

```bash
cargo install backuppc_pool_reader
```

## Usage

### As a binary

The tool propose the following commands:

The command cat will display the content of a file in the pool.

```bash
BPC_TOPDIR=/var/lib/backuppc backuppc_pool_reader cat --host pc-ulrich --number 10 --share /home /ulrich/Downloads/test.txt
```

The command ls will list the content of a directory in the pool.

```bash
BPC_TOPDIR=/var/lib/backuppc backuppc_pool_reader ls pc-ulrich  10 /home /ulrich/Downloads
```

The command host will list all the hostname

```bash
BPC_TOPDIR=/var/lib/backuppc backuppc_pool_reader host
```

The command backups will list all the backups for a host

```bash
BPC_TOPDIR=/var/lib/backuppc backuppc_pool_reader backups pc-ulrich
```

The command mount will mount the pool in a directory to access to all host, backups and share files:

```bash
BPC_TOPDIR=/var/lib/backuppc backuppc_pool_reader mount /tmp/backuppc
```
