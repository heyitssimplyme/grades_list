use std::collections::HashMap;
use structopt::StructOpt;

use scraper::{Html, Selector};
use serde::{Serialize};

use prettytable::{ptable, table, row, cell};

static COURSE_URL: &str = "https://wrem.sis.yorku.ca/Apps/WebObjects/ydml.woa/wa/DirectAction/document?name=CourseListv1";
static LOGIN_PAGE: &str = "https://passportyork.yorku.ca/ppylogin/ppylogin";
static LOGOUT_PAGE: &str = "https://passportyork.yorku.ca/ppylogin/ppylogout";
static USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_6) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0.2 Safari/605.1.15";

#[derive(Debug, StructOpt)]
#[structopt(name = "grades_list", about = "A simple command line program to print out York grades and GPA")]
struct Cli {
  #[structopt(help = "York Username")]
  username: String,
  #[structopt(help = "York Password")]
  password: String,
  #[structopt(short, long, help = "Output in JSON or as a table")]
  json: bool,
}

#[derive(Debug, Serialize)]
struct CourseData {
  session: String,
  course: String,
  title: String,
  grade: String,
}

#[derive(Debug, Serialize)]
struct GPA {
  four: f32,
  nine: f32
}

#[derive(Debug, Serialize)]
struct Output<'a> {
  gpa: &'a GPA,
  grades: &'a Vec<CourseData>,
}

async fn auth (client: &reqwest::Client, args: &Cli) -> Result<bool, Box<dyn std::error::Error>> {
  let resp = client.get(COURSE_URL).send().await?.text().await?;
  
  let mut login_fields: HashMap<String, String> = [
    ("mli".to_owned(), args.username.to_owned()),
    ("password".to_owned(), args.password.to_owned()),
    ("dologin".to_owned(), "Login".to_owned()),
  ].iter().cloned().collect();

  let document = Html::parse_document(&resp);
  let hidden_selector = Selector::parse("input[type='hidden']").unwrap();

  // append all the hiden fields for the auth
  document.select(&hidden_selector).for_each(|element| {
    login_fields.insert(element.value().attr("name").unwrap().to_owned(), element.value().attr("value").unwrap().to_owned());
  });

  let login_resp = client.post(LOGIN_PAGE).form(&login_fields).send().await?;

  let login_resp_content = &login_resp.text().await?;

  // will be authenticated if this string is present in the page
  Ok(login_resp_content.contains("You have successfully authenticated"))
}

async fn scrape_table (client: &reqwest::Client) -> Result<Vec<CourseData>, Box<dyn std::error::Error>> {
  let courses_page = client.get(COURSE_URL).send().await?.text().await?;

  let document = Html::parse_document(&courses_page);
  let table_selector = Selector::parse("table.bodytext").unwrap();
  let tables = document.select(&table_selector).collect::<Vec<_>>();

  if tables.is_empty() {
    panic!("Could not find table!")
  }

  let table = table_extract::Table::find_first(&tables[0].html()).unwrap();

  let mut resp: Vec<CourseData> = Vec::new();

  // they all appear to have &nbsp;
  for row in &table {
    resp.push(CourseData {
      session: row.get("Session").unwrap().to_owned().replace("&nbsp;", ""),
      course: row.get("Course").unwrap().to_owned().replace("&nbsp;", ""),
      title: row.get("Title").unwrap().to_owned().replace("&nbsp;", ""),
      grade: row.get("Grade").unwrap().to_owned().replace("&nbsp;", ""),
    });
  }

  Ok(resp)
}

// calculate both four point and nine point gpa
fn calculate_gpa (grades: &[CourseData]) -> Result<GPA, Box<dyn std::error::Error>> {
  let nine: HashMap<String, f32> = [
    ("A+".to_owned(), 9.0),
    ("A".to_owned(), 8.0),
    ("B+".to_owned(), 7.0),
    ("B".to_owned(), 6.0),
    ("C+".to_owned(), 5.0),
    ("C".to_owned(), 4.0),
    ("D+".to_owned(), 3.0),
    ("D".to_owned(), 2.0),
    ("E".to_owned(), 1.0),
    ("F".to_owned(), 0.0),
  ].iter().cloned().collect();

  let four: HashMap<String, f32> = [
    ("A+".to_owned(), 4.0),
    ("A".to_owned(), 3.8),
    ("B+".to_owned(), 3.3),
    ("B".to_owned(), 3.0),
    ("C+".to_owned(), 2.3),
    ("C".to_owned(), 2.0),
    ("D+".to_owned(), 1.3),
    ("D".to_owned(), 1.0),
    ("E".to_owned(), 0.7),
    ("F".to_owned(), 0.0),
  ].iter().cloned().collect();

  let mut total_credits = 0.0;
  let mut nine_point = 0.0;
  let mut four_point = 0.0;
  for grade in grades {
    if nine.contains_key(&grade.grade) {
      let course_parts = &grade.course.split_ascii_whitespace().collect::<Vec<_>>();
      // parse the credit value
      let credit = course_parts[3].parse::<f32>().unwrap();

      nine_point += nine.get(&grade.grade).unwrap().to_owned() * credit;
      four_point += four.get(&grade.grade).unwrap().to_owned() * credit;

      total_credits += credit;
    }
  }

  Ok(GPA {
    four: four_point / total_credits,
    nine: nine_point / total_credits,
  })
}

async fn logout (client: &reqwest::Client) -> Result<(), Box<dyn std::error::Error>> {
  // a single request is all that is needed
  client.get(LOGOUT_PAGE).send().await?;
  Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>>{
  let args = Cli::from_args();

  let client = reqwest::Client::builder()
    .user_agent(USER_AGENT)
    .cookie_store(true)
    .build()?;

  let authenticated = auth(&client, &args).await?;
  if !authenticated {
    panic!("Could not authenticate!");
  }

  let table = scrape_table(&client).await?;

  logout(&client).await?;

  let gpa = calculate_gpa(&table)?;

  if args.json {
    let output = Output {
      gpa: &gpa,
      grades: &table
    };

    println!("{}", serde_json::to_string(&output).unwrap());
  } else {
    println!("GPA:");
    ptable!(["Four Point", "Nine Point"], [ gpa.four, gpa.nine ]);

    println!();

    println!("Grades:");
    let mut pretty = table!(["Session", "Course", "Title", "Grade"]);

    for row in &table {
      pretty.add_row(row![ row.session, row.course, row.title, row.grade ]);
    }

    pretty.printstd();
  }

  Ok(())
}
