set confirm off
set pagination off
set print pretty on
set print asm-demangle on
set disassemble-next-line on
set breakpoint pending on
set remotetimeout 10
set architecture riscv:rv64
set riscv use-compressed-breakpoints yes
set language rust

file ../target/riscv64gc-unknown-none-elf/release/chp1-demo
target remote localhost:1234

define reload
    file ../target/riscv64gc-unknown-none-elf/release/chp1-demo
end

define regs
    info registers
end

define asm
    x/10i $pc
end

define stack
    x/16gx $sp
end

define entry
    x/20i _start
end

define ni10
    set $i = 0
    while $i < 10
        ni
        x/i $pc
        set $i = $i + 1
    end
end

define rr
    disconnect
    target remote localhost:1234
    reload
end

define boot
    tb _start
    continue
end

define hook-stop
    printf "\n== stop ==\n"
    x/i $pc
    info registers sp ra
end

document reload
Reload the latest chp1-demo ELF symbols.
end

document regs
Show all general registers.
end

document asm
Disassemble 10 instructions from the current PC.
end

document stack
Show 16 doublewords from the current stack pointer.
end

document entry
Disassemble 20 instructions starting at _start.
end

document ni10
Step 10 instructions and show the current instruction after each step.
end

document rr
Reconnect to localhost:1234 and reload the latest ELF symbols.
end

document boot
Set a temporary breakpoint at _start and continue to it.
end
