use std::{thread::sleep, time::Duration};

use anyhow::{Context, Result};
use regex::Regex;
use reqwest::blocking::Client;
use scraper::{Html, Selector};

#[derive(Default, Debug)]
struct PageContacts {
    phones: Vec<String>,
    names: Vec<String>,
}

fn main() -> Result<()> {
    let client = Client::builder()
        .user_agent("Mozilla/5.0 (compatible; list-org-parser/0.1; +https://example.com)")
        .build()
        .context("Не удалось создать HTTP клиент")?;

    let mut id = 1u64;

    loop {
        let contacts = fetch_contacts(&client, id)
            .with_context(|| format!("Ошибка при запросе страницы с id {id}"))?;

        if contacts.phones.is_empty() && contacts.names.is_empty() {
            println!(
                "[{}] На странице нет телефона и ФИО, парсинг остановлен.",
                id
            );
            break;
        }

        println!("ID: {}", id);
        if contacts.phones.is_empty() {
            println!("  Телефоны: нет данных");
        } else {
            for phone in contacts.phones {
                println!("  Телефон: {}", phone);
            }
        }

        if contacts.names.is_empty() {
            println!("  ФИО: нет данных");
        } else {
            for name in contacts.names {
                println!("  ФИО: {}", name);
            }
        }

        println!("----------------------------------------");

        id += 1;
        sleep(Duration::from_millis(300));
    }

    Ok(())
}

fn fetch_contacts(client: &Client, id: u64) -> Result<PageContacts> {
    let url = format!("https://www.list-org.com/company/{id}");
    let response = client
        .get(url)
        .send()
        .context("HTTP запрос завершился неудачно")?
        .error_for_status()
        .context("Сервер вернул ошибочный статус")?;

    let body = response
        .text()
        .context("Не удалось прочитать тело ответа")?;
    let document = Html::parse_document(&body);

    let row_selector = Selector::parse("tr").expect("валидный селектор tr");
    let cell_selector = Selector::parse("td").expect("валидный селектор td");
    let anchor_selector = Selector::parse("a").expect("валидный селектор a");
    let span_selector = Selector::parse("span").expect("валидный селектор span");
    let italic_selector = Selector::parse("p > i").expect("валидный селектор p > i");

    let mut result = PageContacts::default();

    for row in document.select(&row_selector) {
        let mut cells = row.select(&cell_selector);
        let Some(label_cell) = cells.next() else {
            continue;
        };
        let Some(value_cell) = cells.next() else {
            continue;
        };

        let label = normalize_text(&label_cell.text().collect::<Vec<_>>().join(" "));
        let value = normalize_text(&value_cell.text().collect::<Vec<_>>().join(" "));

        let label_lower = label.to_lowercase();
        if label_lower.contains("телефон") {
            for phone in extract_phones(&value) {
                push_unique(&mut result.phones, phone);
            }
        } else if label_lower.contains("руководитель") || label_lower.contains("фио")
        {
            let mut names: Vec<String> = Vec::new();

            for anchor_name in value_cell
                .select(&anchor_selector)
                .map(|a| normalize_text(&a.text().collect::<Vec<_>>().join(" ")))
                .filter(|text| !text.is_empty())
            {
                for name in extract_person_names(&anchor_name) {
                    push_unique(&mut names, name);
                }
            }

            if names.is_empty() && !value.is_empty() {
                for name in extract_person_names(&value) {
                    push_unique(&mut names, name);
                }
            }

            for name in names {
                push_unique(&mut result.names, name);
            }
        }
    }

    for phone_label in document.select(&italic_selector) {
        let label_text = normalize_text(&phone_label.text().collect::<Vec<_>>().join(" "));
        if !label_text.to_lowercase().contains("телефон") {
            continue;
        }

        if let Some(parent) = phone_label.parent().and_then(scraper::ElementRef::wrap) {
            for span in parent.select(&span_selector) {
                let phone = normalize_text(&span.text().collect::<Vec<_>>().join(" "));
                if phone.is_empty() || !phone.chars().any(|c| c.is_ascii_digit()) {
                    continue;
                }
                push_unique(&mut result.phones, phone);
            }
        }
    }

    Ok(result)
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_phones(text: &str) -> Vec<String> {
    let phone_re =
        Regex::new(r"[+]?([\d][\d()\-\s]{4,}\d)").expect("валидное регулярное выражение");
    phone_re
        .find_iter(text)
        .map(|m| normalize_text(m.as_str()))
        .collect()
}

fn extract_person_names(text: &str) -> Vec<String> {
    let fio_re =
        Regex::new(r"[А-ЯЁ][а-яё]+(?:-[А-ЯЁ][а-яё]+)?(?:\s+[А-ЯЁ][а-яё]+(?:-[А-ЯЁ][а-яё]+)?){1,2}")
            .expect("валидное регулярное выражение ФИО");

    let mut names = Vec::new();

    for capture in fio_re.find_iter(text) {
        let name = normalize_text(capture.as_str());
        if !names.iter().any(|n| n == &name) {
            names.push(name);
        }
    }

    if names.is_empty() {
        let normalized = normalize_text(text);
        if !normalized.is_empty() {
            names.push(normalized);
        }
    }

    names
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if !list.iter().any(|v| v == &value) {
        list.push(value);
    }
}
