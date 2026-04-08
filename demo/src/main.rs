mod trait_dispatch;

fn main() {
    println!("========================================");
    println!(" Trait & Dispatch Demo");
    println!("========================================\n");
    trait_dispatch::run();

    // 后续添加新主题时，按如下模式扩展：
    // mod ownership;
    // ownership::run();
}
