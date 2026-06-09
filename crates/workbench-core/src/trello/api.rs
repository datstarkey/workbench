use anyhow::{bail, Context, Result};

use std::collections::HashSet;

use super::types::{
    TrelloBoard, TrelloBoardData, TrelloCard, TrelloColumnData, TrelloCredentials, TrelloLabel,
    TrelloList, TrelloOrganization,
};

const BASE_URL: &str = "https://api.trello.com/1";

pub async fn validate_auth(creds: &TrelloCredentials) -> Result<bool> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{BASE_URL}/members/me"))
        .query(&[("key", &creds.api_key), ("token", &creds.token)])
        .send()
        .await
        .context("Failed to connect to Trello API")?;

    Ok(resp.status().is_success())
}

pub async fn list_boards(creds: &TrelloCredentials) -> Result<Vec<TrelloBoard>> {
    let client = reqwest::Client::new();

    // Fetch boards where user is a direct member
    let resp = client
        .get(format!("{BASE_URL}/members/me/boards"))
        .query(&[
            ("key", creds.api_key.as_str()),
            ("token", creds.token.as_str()),
            ("fields", "id,name,url"),
            ("filter", "open"),
        ])
        .send()
        .await
        .context("Failed to list Trello boards")?;

    if !resp.status().is_success() {
        bail!("Trello API error listing boards: {}", resp.status());
    }

    let mut boards: Vec<TrelloBoard> =
        resp.json().await.context("Failed to parse boards response")?;
    let mut seen: HashSet<String> = boards.iter().map(|b| b.id.clone()).collect();

    // Also fetch boards from each organization/workspace (includes workspace-visible boards)
    let orgs = list_organizations(creds).await.unwrap_or_default();
    for org in &orgs {
        if let Ok(org_boards) = list_organization_boards(creds, &org.id).await {
            for board in org_boards {
                if seen.insert(board.id.clone()) {
                    boards.push(board);
                }
            }
        }
    }

    boards.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(boards)
}

async fn list_organizations(creds: &TrelloCredentials) -> Result<Vec<TrelloOrganization>> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{BASE_URL}/members/me/organizations"))
        .query(&[
            ("key", creds.api_key.as_str()),
            ("token", creds.token.as_str()),
            ("fields", "id"),
        ])
        .send()
        .await
        .context("Failed to list Trello organizations")?;

    if !resp.status().is_success() {
        bail!("Trello API error listing organizations: {}", resp.status());
    }

    resp.json()
        .await
        .context("Failed to parse organizations response")
}

async fn list_organization_boards(
    creds: &TrelloCredentials,
    org_id: &str,
) -> Result<Vec<TrelloBoard>> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{BASE_URL}/organizations/{org_id}/boards"))
        .query(&[
            ("key", creds.api_key.as_str()),
            ("token", creds.token.as_str()),
            ("fields", "id,name,url"),
            ("filter", "open"),
        ])
        .send()
        .await
        .context("Failed to list organization boards")?;

    if !resp.status().is_success() {
        bail!(
            "Trello API error listing org boards: {}",
            resp.status()
        );
    }

    resp.json()
        .await
        .context("Failed to parse org boards response")
}

pub async fn list_columns(creds: &TrelloCredentials, board_id: &str) -> Result<Vec<TrelloList>> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{BASE_URL}/boards/{board_id}/lists"))
        .query(&[
            ("key", creds.api_key.as_str()),
            ("token", creds.token.as_str()),
            ("fields", "id,name,pos"),
            ("filter", "open"),
        ])
        .send()
        .await
        .context("Failed to list Trello columns")?;

    if !resp.status().is_success() {
        bail!(
            "Trello API error listing columns: {}",
            resp.status()
        );
    }

    let lists: Vec<TrelloList> = resp.json().await.context("Failed to parse columns response")?;
    Ok(lists)
}

pub async fn list_cards(creds: &TrelloCredentials, list_id: &str) -> Result<Vec<TrelloCard>> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{BASE_URL}/lists/{list_id}/cards"))
        .query(&[
            ("key", creds.api_key.as_str()),
            ("token", creds.token.as_str()),
            ("fields", "id,name,desc,idList,url,labels,pos,due"),
        ])
        .send()
        .await
        .context("Failed to list Trello cards")?;

    if !resp.status().is_success() {
        bail!(
            "Trello API error listing cards: {}",
            resp.status()
        );
    }

    let cards: Vec<TrelloCard> = resp.json().await.context("Failed to parse cards response")?;
    Ok(cards)
}

pub async fn list_labels(creds: &TrelloCredentials, board_id: &str) -> Result<Vec<TrelloLabel>> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{BASE_URL}/boards/{board_id}/labels"))
        .query(&[("key", &creds.api_key), ("token", &creds.token)])
        .send()
        .await
        .context("Failed to list Trello labels")?;

    if !resp.status().is_success() {
        bail!(
            "Trello API error listing labels: {}",
            resp.status()
        );
    }

    let labels: Vec<TrelloLabel> =
        resp.json().await.context("Failed to parse labels response")?;
    Ok(labels)
}

pub async fn create_card(
    creds: &TrelloCredentials,
    list_id: &str,
    name: &str,
    desc: Option<&str>,
) -> Result<TrelloCard> {
    let client = reqwest::Client::new();
    let mut query = vec![
        ("key", creds.api_key.as_str()),
        ("token", creds.token.as_str()),
        ("idList", list_id),
        ("name", name),
    ];
    if let Some(d) = desc {
        query.push(("desc", d));
    }

    let resp = client
        .post(format!("{BASE_URL}/cards"))
        .query(&query)
        .send()
        .await
        .context("Failed to create Trello card")?;

    if !resp.status().is_success() {
        bail!(
            "Trello API error creating card: {}",
            resp.status()
        );
    }

    let card: TrelloCard = resp.json().await.context("Failed to parse created card response")?;
    Ok(card)
}

pub async fn move_card(
    creds: &TrelloCredentials,
    card_id: &str,
    target_list_id: &str,
) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .put(format!("{BASE_URL}/cards/{card_id}"))
        .query(&[
            ("key", creds.api_key.as_str()),
            ("token", creds.token.as_str()),
            ("idList", target_list_id),
        ])
        .send()
        .await
        .context("Failed to move Trello card")?;

    if !resp.status().is_success() {
        bail!(
            "Trello API error moving card: {}",
            resp.status()
        );
    }

    Ok(())
}

pub async fn add_label_to_card(
    creds: &TrelloCredentials,
    card_id: &str,
    label_id: &str,
) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{BASE_URL}/cards/{card_id}/idLabels"))
        .query(&[
            ("key", creds.api_key.as_str()),
            ("token", creds.token.as_str()),
            ("value", label_id),
        ])
        .send()
        .await
        .context("Failed to add label to Trello card")?;

    if !resp.status().is_success() {
        bail!(
            "Trello API error adding label: {}",
            resp.status()
        );
    }

    Ok(())
}

pub async fn remove_label_from_card(
    creds: &TrelloCredentials,
    card_id: &str,
    label_id: &str,
) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .delete(format!("{BASE_URL}/cards/{card_id}/idLabels/{label_id}"))
        .query(&[
            ("key", creds.api_key.as_str()),
            ("token", creds.token.as_str()),
        ])
        .send()
        .await
        .context("Failed to remove label from Trello card")?;

    if !resp.status().is_success() {
        bail!(
            "Trello API error removing label: {}",
            resp.status()
        );
    }

    Ok(())
}

pub async fn fetch_board_data(
    creds: &TrelloCredentials,
    board_id: &str,
    hidden_columns: &[String],
) -> Result<TrelloBoardData> {
    let client = reqwest::Client::new();

    // Fetch board info
    let resp = client
        .get(format!("{BASE_URL}/boards/{board_id}"))
        .query(&[
            ("key", creds.api_key.as_str()),
            ("token", creds.token.as_str()),
            ("fields", "id,name,url"),
        ])
        .send()
        .await
        .context("Failed to fetch Trello board")?;

    if !resp.status().is_success() {
        bail!(
            "Trello API error fetching board: {}",
            resp.status()
        );
    }

    let board: TrelloBoard = resp.json().await.context("Failed to parse board response")?;

    // Fetch all columns
    let all_columns = list_columns(creds, board_id).await?;

    // Filter out hidden columns and fetch cards for visible ones
    let mut columns = Vec::new();
    for col in all_columns {
        if hidden_columns.contains(&col.id) {
            continue;
        }
        let cards = list_cards(creds, &col.id).await?;
        columns.push(TrelloColumnData {
            column: col,
            cards,
        });
    }

    Ok(TrelloBoardData { board, columns })
}
