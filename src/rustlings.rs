#[macro_use]
mod macros {
    macro_rules! my_macro {
        () => {
            println!("Check out my macro!");
        };
    }
}


pub fn test_main() {
    println!("Test");

    /*let name = String::from("Jill Smith");
    let title = String::from("Fish Flying");
    let book = Book { author: &name, title: &title };

    println!("{} by {}", book.title, book.author);
    println!("Name: {}", name);*/

    my_macro!();
}


/*struct Book<'a> {
    author: &'a str,
    title: &'a str,
}*/
