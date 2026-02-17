use std::io;

fn main() {
    println!("Simple Calculator");
    println!("Supported operations: +, -, *, /, sqrt, radical");
    println!("Enter 'quit' to exit");
    
    loop {
        println!("\nEnter an expression (e.g., 2 + 3 or sqrt 16 or radical 3 27): ");
        let mut input = String::new();
        io::stdin().read_line(&mut input).expect("Failed to read input");
        
        let input = input.trim();
        
        if input == "quit" {
            println!("Goodbye!");
            break;
        }
        
        match evaluate_expression(input) {
            Ok(result) => println!("Result: {}", result),
            Err(e) => println!("Error: {}", e),
        }
    }
}

fn evaluate_expression(expression: &str) -> Result<f64, String> {
    let parts: Vec<&str> = expression.split_whitespace().collect();
    
    if parts.len() == 2 && parts[0] == "sqrt" {
        // Handle square root operation
        let num = parts[1].parse::<f64>();
        match num {
            Ok(n) => {
                if n < 0.0 {
                    Err("Cannot calculate square root of negative number".to_string())
                } else {
                    Ok(n.sqrt())
                }
            }
            Err(_) => Err("Invalid number for square root".to_string()),
        }
    } else if parts.len() == 3 {
        // Handle binary operations
        let num1 = parts[0].parse::<f64>();
        let num2 = parts[2].parse::<f64>();
        let operator = parts[1];
        
        match (num1, num2) {
            (Ok(n1), Ok(n2)) => {
                match operator {
                    "+" => Ok(n1 + n2),
                    "-" => Ok(n1 - n2),
                    "*" => Ok(n1 * n2),
                    "/" => {
                        if n2 == 0.0 {
                            Err("Division by zero error".to_string())
                        } else {
                            Ok(n1 / n2)
                        }
                    }
                    _ => Err(format!("Unsupported operator: {}", operator)),
                }
            }
            (Err(_), _) => Err("First operand is not a valid number".to_string()),
            (_, Err(_)) => Err("Second operand is not a valid number".to_string()),
        }
    } else {
        Err("Invalid expression format. Expected: number operator number or sqrt number".to_string())
    }
}