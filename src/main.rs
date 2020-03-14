#[allow(dead_code)]
mod util;

extern crate csv;
extern crate curl;
extern crate regex;

use curl::easy::{Easy2, Handler, WriteError};
use regex::Regex;
use std::collections::HashMap;
use std::io;
use std::io::Read;
use termion::event::Key;
use termion::input::MouseTerminal;
use termion::raw::IntoRawMode;
use termion::screen::AlternateScreen;
use tui::backend::TermionBackend;
use tui::layout::{Alignment, Constraint, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{Axis, Block, Borders, Chart, Dataset, Marker, Paragraph, Row, Table, Text};
use tui::Terminal;

use crate::util::{Event, Events};

#[derive(PartialEq)]
enum DataType {
    Confirmed,
    Deaths,
    Recovered,
}

static CONFIRMED_URL: &str = "https://raw.githubusercontent.com/CSSEGISandData/COVID-19/master/csse_covid_19_data/csse_covid_19_time_series/time_series_19-covid-Confirmed.csv";
static DEATHS_URL: &str = "https://raw.githubusercontent.com/CSSEGISandData/COVID-19/master/csse_covid_19_data/csse_covid_19_time_series/time_series_19-covid-Deaths.csv";
static RECOVERED_URL: &str = "https://raw.githubusercontent.com/CSSEGISandData/COVID-19/master/csse_covid_19_data/csse_covid_19_time_series/time_series_19-covid-Recovered.csv";

struct Collector(Vec<u8>);

struct Country {
    country: String,
    confirmed: u32,
    deaths: u32,
    recovered: u32,
    confirmed_map: Vec<u32>,
    deaths_map: Vec<u32>,
    recovered_map: Vec<u32>,
    headers: Vec<String>,
}

struct App {
    selected: usize,
}

impl App {
    fn new() -> App {
        App {
            selected: 0,
        }
    }
}

impl Country {
    fn new(country: String, confirmed: u32, deaths: u32, recovered: u32,
           confirmed_map: Vec<u32>, deaths_map: Vec<u32>, recovered_map: Vec<u32>,
           headers: Vec<String>) -> Country {
        Country {
            country, confirmed, deaths, recovered, confirmed_map, deaths_map, recovered_map, headers
        }
    }

    fn get_row(&self) -> Vec<String> {
        vec![
            self.country.to_string(),
            self.confirmed.to_string(),
            self.deaths.to_string(),
            format!("{:.2}%", get_percentage(self.deaths, self.confirmed)),
            self.recovered.to_string(),
            format!("{:.2}%", get_percentage(self.recovered, self.confirmed))
        ]
    }
}

impl Handler for Collector {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        self.0.extend_from_slice(data);
        Ok(data.len())
    }
}

fn get_csv_from_url(url: String) -> String {
    let mut easy = Easy2::new(Collector(Vec::new()));
    easy.get(true).unwrap();
    easy.url(&url).unwrap();
    easy.perform().unwrap();

    assert_eq!(easy.response_code().unwrap(), 200);
    let easy_ref = easy.get_ref();
    let content = std::str::from_utf8(&easy_ref.0).unwrap();
    content.to_string()
}

fn get_percentage(part: u32, total: u32) -> f32 {
    return 100f32 * (part as f32 / total as f32);
}

fn sum_vectors(source: &Vec<u32>, dest: &mut Vec<u32>) {
    let mut source_iter = source.iter();
    for i in dest.iter_mut() {
        *i += *source_iter.next().unwrap();
    }
}

fn get_header_map<R: Read>(reader: &mut csv::Reader<R>) -> Result<(Vec<String>, usize), csv::Error> {
    let headers = reader.headers()?;
    let header_count = headers.len();
    let re = Regex::new(r"^\d{1,2}/\d{1,2}/\d{2}$").unwrap();
    let h_map: Vec<_> = headers.iter().filter(|h| re.is_match(h)).collect();
    let mut header_map: Vec<String> = Vec::new();

    for h in &h_map {
        header_map.push(h.to_string());
    }

    Ok((header_map, header_count))
}

fn get_results<R: Read>(reader: &mut csv::Reader<R>, countries_map: &mut HashMap<String, Country>, header_count: usize, data_type: DataType) -> Result<(), csv::Error> {
    for result in reader.records() {
        let record = result?;
        let country = &record[1];
        let mut confirmed: u32 = 0;
        let mut deaths: u32 = 0;
        let mut recovered: u32 = 0;
        let mut confirmed_list: Vec<u32> = Vec::new();
        let mut deaths_list: Vec<u32> = Vec::new();
        let mut recovered_list: Vec<u32> = Vec::new();

        for i in 4..header_count {
            let value: u32 = record[i].parse().unwrap();

            if data_type == DataType::Confirmed {
                confirmed = value;
                confirmed_list.push(value);
            } else if data_type == DataType::Deaths {
                deaths = value;
                deaths_list.push(value);
            } else if data_type == DataType::Recovered {
                recovered = value;
                recovered_list.push(value);
            }
        }

        if countries_map.contains_key(country) {
            let old_value = countries_map.get(country).unwrap();
            confirmed += old_value.confirmed;
            deaths += old_value.deaths;
            recovered += old_value.recovered;

            // Add to existing values
            if data_type == DataType::Confirmed {
                deaths_list = old_value.deaths_map.to_vec();
                recovered_list = old_value.recovered_map.to_vec();
                sum_vectors(&old_value.confirmed_map, &mut confirmed_list);
            } else if data_type == DataType::Deaths {
                confirmed_list = old_value.confirmed_map.to_vec();
                recovered_list = old_value.recovered_map.to_vec();

                if old_value.deaths_map.len() > 0 {
                    sum_vectors(&old_value.deaths_map, &mut deaths_list);
                }
            } else if data_type == DataType::Recovered {
                confirmed_list = old_value.confirmed_map.to_vec();
                deaths_list = old_value.deaths_map.to_vec();

                if old_value.recovered_map.len() > 0 {
                    sum_vectors(&old_value.recovered_map, &mut recovered_list);
                }    
            }
        }

        countries_map.insert(country.to_string(), Country::new(country.to_string(),
                             confirmed, deaths, recovered, confirmed_list, deaths_list, recovered_list, Vec::new()));
    }
    
    Ok(())
}

// Adds a "TOTAL" summary country to the HashMap that contains summed results
fn add_summary(countries_map: &mut HashMap<String, Country>, header_map: Vec<String>) {
    let mut total_confirmed: u32 = 0;
    let mut total_deaths: u32 = 0;
    let mut total_recovered: u32 = 0;
    let mut total_confirmed_map: Vec<u32> = Vec::new();
    let mut total_deaths_map: Vec<u32> = Vec::new();
    let mut total_recovered_map: Vec<u32> = Vec::new();
    let mut list: Vec<_> = countries_map.iter().collect();

    list.sort_by(|a, b| b.1.confirmed.cmp(&a.1.confirmed));
    for (_c, v) in &list {
        total_confirmed += v.confirmed;
        total_deaths += v.deaths;
        total_recovered += v.recovered;

        // Init vectors if needed
        if total_confirmed_map.len() == 0 {
            total_confirmed_map = vec![0; v.confirmed_map.len()];
        }

        if total_deaths_map.len() == 0 {
            total_deaths_map = vec![0; v.deaths_map.len()];
        }

        if total_recovered_map.len() == 0 {
            total_recovered_map = vec![0; v.recovered_map.len()];
        }

        sum_vectors(&v.confirmed_map, &mut total_confirmed_map);
        sum_vectors(&v.deaths_map, &mut total_deaths_map);
        sum_vectors(&v.recovered_map, &mut total_recovered_map);
    }

    countries_map.insert("TOTAL".to_string(), Country::new("TOTAL".to_string(),
                         total_confirmed, total_deaths, total_recovered,
                         total_confirmed_map, total_deaths_map, total_recovered_map, header_map));
}

fn get_data() -> Result<HashMap<String, Country>, csv::Error> {
    // TODO: async
    let content_conf = get_csv_from_url(CONFIRMED_URL.to_string());
    let content_deaths = get_csv_from_url(DEATHS_URL.to_string());
    let content_recov = get_csv_from_url(RECOVERED_URL.to_string());
    let mut reader_conf = csv::Reader::from_reader(content_conf.as_bytes());
    let mut reader_deaths = csv::Reader::from_reader(content_deaths.as_bytes());
    let mut reader_recov = csv::Reader::from_reader(content_recov.as_bytes());
    
    let mut countries_map: HashMap<String, Country> = HashMap::new();
    let (header_map, header_count) = get_header_map(&mut reader_conf).unwrap();

    get_results(&mut reader_conf, &mut countries_map, header_count, DataType::Confirmed).unwrap();
    get_results(&mut reader_deaths, &mut countries_map, header_count, DataType::Deaths).unwrap();
    get_results(&mut reader_recov, &mut countries_map, header_count, DataType::Recovered).unwrap();
    add_summary(&mut countries_map, header_map);

    Ok(countries_map)
}

fn get_table_columns(countries_map: &HashMap<String, Country>, sorty_by: DataType) -> Vec<Vec<String>> {
    let mut table = Vec::new();
    let mut columns: Vec<_> = countries_map.iter().collect();

    match sorty_by {
        DataType::Confirmed => columns.sort_by(|a, b| b.1.confirmed.cmp(&a.1.confirmed)),
        DataType::Deaths => columns.sort_by(|a, b| b.1.deaths.cmp(&a.1.deaths)),
        DataType::Recovered => columns.sort_by(|a, b| b.1.recovered.cmp(&a.1.recovered)),
    }

    for (_c, v) in &columns {
        table.push(v.get_row());
    }

    table
}

fn get_chart_from_country(countries_map: &HashMap<String, Country>, country: String) -> (Vec<(f64, f64)>, Vec<(f64, f64)>, Vec<(f64, f64)>) {
    let selected_country = &countries_map.get(&country).unwrap();
    let confirmed_data = &selected_country.confirmed_map;
    let deaths_data = &selected_country.deaths_map;
    let recovered_data = &selected_country.recovered_map;

    let mut confirmed_final: Vec<(f64, f64)> = Vec::new();
    let mut deaths_final: Vec<(f64, f64)> = Vec::new();
    let mut recovered_final: Vec<(f64, f64)> = Vec::new();

    for (i, x) in confirmed_data.iter().enumerate() {
        confirmed_final.push((i as f64, *x as f64));
    }

    for (i, x) in deaths_data.iter().enumerate() {
        deaths_final.push((i as f64, *x as f64));
    }

    for (i, x) in recovered_data.iter().enumerate() {
        recovered_final.push((i as f64, *x as f64));
    }

    (confirmed_final, deaths_final, recovered_final)
}

fn update_data(countries_map: &HashMap<String, Country>, current_table: &Vec<Vec<String>>, selected: usize, 
          confirmed_data: &mut Vec<(f64, f64)>, deaths_data: &mut Vec<(f64, f64)>, recovered_data: &mut Vec<(f64, f64)>) -> String {
    let selected_country = current_table.get(selected).unwrap().first().unwrap().to_string();
    let (confirmed_upd, deaths_upd, recovered_upd) = get_chart_from_country(&countries_map, selected_country.to_string());
    *confirmed_data = confirmed_upd;
    *deaths_data = deaths_upd;
    *recovered_data = recovered_upd;

    selected_country
}

fn main() -> Result<(), failure::Error> {
    println!("Loading CSV data...");
    let countries_map = get_data().unwrap();
    let total = countries_map.get("TOTAL").unwrap();

    let summary = [
        Text::raw(format!("Updated: {}\n", total.headers.last().unwrap())),
        Text::raw(format!("Total confirmed: {}\n", total.confirmed)),
        Text::raw(format!("Total deaths: {} ({:.2}%)\n", total.deaths, get_percentage(total.deaths, total.confirmed))),
        Text::raw(format!("Total recovered: {} ({:.2}%)\n", total.recovered, get_percentage(total.recovered, total.confirmed))),
    ];

    let (mut confirmed_data, mut deaths_data, mut recovered_data) = get_chart_from_country(&countries_map, "TOTAL".to_string());
    let mut current_table = get_table_columns(&countries_map, DataType::Confirmed);

    let stdout = io::stdout().into_raw_mode()?;
    let stdout = MouseTerminal::from(stdout);
    let stdout = AlternateScreen::from(stdout);
    let backend = TermionBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let events = Events::new();
    let mut app = App::new();
    let mut selected_country = String::from("TOTAL");

    loop {
        terminal.draw(|mut f| {
            let selected_style = Style::default().fg(Color::Yellow).modifier(Modifier::BOLD);
            let normal_style = Style::default().fg(Color::White);
            let header = ["Country", "Confirmed", "Deaths", "Deaths (%)", "Recovered", "Recovered (%)"];

            let rects = Layout::default()
                .direction(Direction::Vertical)
                .constraints(
                    [
                        Constraint::Percentage(45),
                        Constraint::Percentage(45),
                        Constraint::Length(6) // How to really make this fixed height?
                    ].as_ref())
                .margin(0)
                .split(f.size());

            let offset = rects[0]
                .height
                .checked_sub(5)
                .and_then(|height| app.selected.checked_sub(height as usize))
                .unwrap_or(0);

            let rows = current_table.iter().skip(offset).enumerate().map(|(i, item)| {
                if i == app.selected - offset {
                    Row::StyledData(item.into_iter(), selected_style)
                } else {
                    Row::StyledData(item.into_iter(), normal_style)
                }
            });

            // Set column widths to 15 (max) or 15%
            let widths = header
                .iter()
                .map(|_h| if rects[0].width > 100 { Constraint::Length(15) } else { Constraint::Percentage(15) })
                .collect::<Vec<tui::layout::Constraint>>();

            let mut table = Table::new(header.iter(), rows)
                .block(Block::default().borders(Borders::ALL).title("Corona virus - Sort by: (c) confirmed, (d) deaths, (r) recovered"))
                .widths(&widths);
            f.render(&mut table, rects[0]);

            let datasets = [
                Dataset::default()
                    .name("Confirmed")
                    .marker(Marker::Braille)
                    .style(Style::default().fg(Color::Cyan))
                    .data(&confirmed_data),
                Dataset::default()
                    .name("Deaths")
                    .marker(Marker::Braille)
                    .style(Style::default().fg(Color::Red))
                    .data(&deaths_data),
                Dataset::default()
                    .name("Recovered")
                    .marker(Marker::Braille)
                    .style(Style::default().fg(Color::Yellow))
                    .data(&recovered_data),
            ];

            // Labels. Put some extra to the end for empty space (btw. There's too many labels, so tui-rs decided not to show them at all)
            let mut x_labels = total.headers.to_vec();
            x_labels.extend(vec!["00/00/00".to_string(), "00/00/00".to_string(), "00/00/00".to_string(), "00/00/00".to_string()]);
        
            // This is damn ugly. But it keeps the y_bounds a little larger than the maximum.
            let x_bounds = [0.0, x_labels.len() as f64];
            let max = confirmed_data.last().unwrap().1 as f64 + 1.0;
            let y_bounds = [0.0, max * 1.2];
            let y_labels = ["0", &((max / 2.0) as u32).to_string(), &((max * 1.2) as u32).to_string()];

            let mut chart = Chart::default()
                .block(
                    Block::default()
                        .title(&selected_country)
                        .title_style(Style::default().fg(Color::Gray).modifier(Modifier::BOLD))
                        .borders(Borders::ALL),
                )
                .x_axis(
                    Axis::default()
                        .title("Date")
                        .style(Style::default().fg(Color::Gray))
                        .labels_style(Style::default().modifier(Modifier::ITALIC))
                        .bounds(x_bounds)
                        .labels(&x_labels),
                )
                .y_axis(
                    Axis::default()
                        .title("Count")
                        .style(Style::default().fg(Color::Gray))
                        .labels_style(Style::default().modifier(Modifier::ITALIC))
                        .bounds(y_bounds)
                        .labels(&y_labels),
                )
                .datasets(&datasets);
            f.render(&mut chart, rects[1]);

            let mut footer = Paragraph::new(summary.iter())
                    .block(Block::default().title("Summary").borders(Borders::ALL))
                    .alignment(Alignment::Left)
                    .wrap(true);
            f.render(&mut footer, rects[2]);
        })?;

        match events.next()? {
            Event::Input(key) => match key {
                Key::Char('q') => {
                    break;
                }
                Key::Esc => {
                    break;
                }
                Key::Down => {
                    app.selected += 1;
                    if app.selected > current_table.len() - 1 {
                        app.selected = 0;
                    }
                    selected_country = update_data(&countries_map, &current_table, app.selected, &mut confirmed_data, &mut deaths_data, &mut recovered_data);
                }
                Key::Up => {
                    if app.selected > 0 {
                        app.selected -= 1;
                    } else {
                        app.selected = current_table.len() - 1;
                    }
                    selected_country = update_data(&countries_map, &current_table, app.selected, &mut confirmed_data, &mut deaths_data, &mut recovered_data);
                }
                Key::Char('c') => {
                    current_table = get_table_columns(&countries_map, DataType::Confirmed);
                    selected_country = update_data(&countries_map, &current_table, app.selected, &mut confirmed_data, &mut deaths_data, &mut recovered_data);
                }
                Key::Char('d') => {
                    current_table = get_table_columns(&countries_map, DataType::Deaths);
                    selected_country = update_data(&countries_map, &current_table, app.selected, &mut confirmed_data, &mut deaths_data, &mut recovered_data);
                }
                Key::Char('r') => {
                    current_table = get_table_columns(&countries_map, DataType::Recovered);
                    selected_country = update_data(&countries_map, &current_table, app.selected, &mut confirmed_data, &mut deaths_data, &mut recovered_data);
                }
                _ => {}
            },
            _ => {}
        };
    }

    Ok(())
}
