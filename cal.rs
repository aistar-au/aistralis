use std::io;

fn main() {
    println!("Simple Calculator");
    println!("Enter operations in the format: number1 operator number2");
    println!("Supported operators: +, -, *, /, sqrt");
    println!("Enter 'quit' to exit");
    
    loop {
        println!("\nEnter calculation:");
        let mut input = String::new();
        io::stdin().read_line(&mut input).expect("Failed to read input");
        
        let input = input.trim();
        
        if input == "quit" {
            println!("Goodbye!");
            break;
        }
        
        let result = calculate(input);
        match result {
            Ok(value) => println!("Result: {}", value),
            Err(e) => println!("Error: {}", e),
        }
    }
}

fn calculate(input: &str) -> Result<f64, String> {
    let parts: Vec<&str> = input.split_whitespace().collect();
    
    if parts.len() == 3 {
        // Standard binary operations
        let num1: f64 = parts[0].parse().map_err(|_| "First value is not a valid number".to_string())?;
        let operator = parts[1];
        let num2: f64 = parts[2].parse().map_err(|_| "Second value is not a valid number".to_string())?;
        
        match operator {
            "-" => Ok(num1 - num2),
            "*" => Ok(num1 * num2),
            "/" => {
                if num2 == 0.0 {
                    Err("Division by zero is not allowed".to_string())
                } else {
                    Ok(num1 / num2)
                }
            }
            _ => Err("Unsupported operator. Use -, *, or /".to_string()),
        }
    } else if parts.len() == 2 && parts[0] == "sqrt" {
        // Square root operation
        let num: f64 = parts[1].parse().map_err(|_| "Invalid number for square root".to_string())?;
        if num < 0.0 {
            Err("Square root of negative number is not allowed".to_string())
        } else {
            Ok(num.sqrt())
        }
    } else {
        Err("Invalid input format. Use: number operator number or 'sqrt number'".to_string())
    }
}