mod network_package;
use network_package::object::NetworkPackage;
use network_package::object::NetworkPackageData;

fn main() {
    let a = NetworkPackage::Hello(b"Some text".to_vec());
    let x = match &a {
        NetworkPackage::Hello(x) => x,
        _ => panic!("invalid object"),
    };
    println!("Hello, world! {:?}", x);
}
