use std::{thread::sleep, time::Duration};

use anyhow::{Context, Result};
use rand::{seq::SliceRandom, Rng};
use regex::Regex;
use reqwest::{
    blocking::Client,
    header::{REFERER, USER_AGENT},
};
use scraper::{Html, Selector};

#[derive(Default, Debug)]
struct PageContacts {
    phones: Vec<String>,
    names: Vec<String>,
}

fn main() -> Result<()> {
    let client = Client::builder()
        .build()
        .context("Не удалось создать HTTP клиент")?;

    let user_agents = vec![
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15",
        "Mozilla/5.0 (X11; Linux x86_64; rv:125.0) Gecko/20100101 Firefox/125.0",
    ];

    let referers = vec![
        "https://www.google.com/",
        "https://yandex.ru/",
        "https://www.bing.com/",
    ];

    let mut id = 1u64;
    let mut rng = rand::thread_rng();

    loop {
        let user_agent = user_agents
            .choose(&mut rng)
            .expect("список user-agent не пуст")
            .to_string();
        let referer = referers.choose(&mut rng).map(|s| *s);

        let contacts = fetch_contacts(&client, id, &user_agent, referer)
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
        let delay_ms = rng.gen_range(750..=2000);
        sleep(Duration::from_millis(delay_ms));
    }

    Ok(())
}

fn fetch_contacts(
    client: &Client,
    id: u64,
    user_agent: &str,
    referer: Option<&str>,
) -> Result<PageContacts> {
    let url = format!("https://www.list-org.com/company/{id}");
    let mut request = client.get(url).header(USER_AGENT, user_agent);

    if let Some(referer) = referer {
        request = request.header(REFERER, referer);
    }

    let response = request
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
                .filter(|anchor| !is_element_hidden(anchor))
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

fn is_element_hidden(element: &scraper::ElementRef) -> bool {
    let value = element.value();

    if value.attr("hidden").is_some() {
        return true;
    }

    if let Some(style) = value.attr("style") {
        let style_lower = style.to_ascii_lowercase();
        if style_lower.contains("display:none")
            || style_lower.contains("visibility:hidden")
            || style_lower.contains("opacity:0")
            || style_lower.contains("opacity: 0")
        {
            return true;
        }
    }

    if let Some(aria_hidden) = value.attr("aria-hidden") {
        if aria_hidden.eq_ignore_ascii_case("true") {
            return true;
        }
    }

    false
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if !list.iter().any(|v| v == &value) {
        list.push(value);
    }
}
