//! Category-only changes never rewrite the original diary record or its receipt.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryDeletePreview {
    pub category: Category,
    pub entry_ids: Vec<String>,
}

const CHANGED: &str = "카테고리나 연결된 기록이 변경되었습니다. 최신 내용을 다시 확인해 주세요.";

fn require_custom(id: &str) -> Result<(), String> {
    validate_id(id)?;
    if [SCHEDULE_CATEGORY, RECORD_CATEGORY].contains(&id) {
        return Err("기본 카테고리는 삭제할 수 없습니다.".into());
    }
    Ok(())
}

fn preview(c: &Connection, id: &str) -> Result<CategoryDeletePreview, String> {
    require_custom(id)?;
    let category = categories(c)?
        .into_iter()
        .find(|v| v.id == id)
        .ok_or(CHANGED)?;
    let mut statement = c
        .prepare("SELECT entry_id FROM entry_display WHERE category_id=?1 ORDER BY entry_id")
        .map_err(database_error)?;
    let entry_ids = statement
        .query_map([id], |r| r.get(0))
        .map_err(database_error)?
        .collect::<Result<Vec<String>, _>>()
        .map_err(database_error)?;
    Ok(CategoryDeletePreview {
        category,
        entry_ids,
    })
}

impl Storage {
    pub fn preview_category_deletion(&self, id: &str) -> Result<CategoryDeletePreview, String> {
        let mut c = self.connection()?;
        let tx = c.transaction().map_err(database_error)?;
        let result = preview(&tx, id)?;
        tx.commit().map_err(database_error)?;
        Ok(result)
    }

    pub fn delete_category(
        &self,
        expected: &CategoryDeletePreview,
        target: Option<&Category>,
    ) -> Result<usize, String> {
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        let current = preview(&tx, &expected.category.id)?;
        if current != *expected {
            return Err(CHANGED.into());
        }
        if !current.entry_ids.is_empty() && target.is_none() {
            return Err("기록을 옮길 카테고리를 선택해 주세요.".into());
        }
        if let Some(target) = target {
            if target.id == current.category.id || target.id == SCHEDULE_CATEGORY {
                return Err("기록을 옮길 다른 카테고리를 선택해 주세요.".into());
            }
            if !categories(&tx)?.contains(target) {
                return Err(CHANGED.into());
            }
            tx.execute(
                "UPDATE entry_display SET category_id=?1 WHERE category_id=?2",
                params![target.id, current.category.id],
            )
            .map_err(database_error)?;
        }
        tx.execute(
            "DELETE FROM calendar_categories WHERE id=?1",
            [&current.category.id],
        )
        .map_err(database_error)?;
        for (position, category) in categories(&tx)?.iter().enumerate() {
            tx.execute(
                "UPDATE calendar_categories SET position=?1 WHERE id=?2",
                params![position, category.id],
            )
            .map_err(database_error)?;
        }
        tx.commit().map_err(database_error)?;
        Ok(current.entry_ids.len())
    }

    pub fn update_category_checked(
        &self,
        expected: &Category,
        name: &str,
        color: &str,
    ) -> Result<Category, String> {
        let mut c = self.connection()?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        if !categories(&tx)?.contains(expected) {
            return Err(CHANGED.into());
        }
        let result = write_category(&tx, Some(&expected.id), name, color)?;
        tx.commit().map_err(database_error)?;
        Ok(result)
    }
}

pub(super) fn write_category(
    c: &Connection,
    id: Option<&str>,
    name: &str,
    color: &str,
) -> Result<Category, String> {
    let name = line(name, "카테고리 이름", 32)?;
    if name.is_empty() {
        return Err("카테고리 이름을 입력해 주세요.".into());
    }
    if color.len() != 7
        || !color.starts_with('#')
        || !color[1..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("색상은 #RRGGBB 형식으로 입력해 주세요.".into());
    }
    let all = categories(c)?;
    if all
        .iter()
        .any(|v| Some(v.id.as_str()) != id && v.name.to_lowercase() == name.to_lowercase())
    {
        return Err("같은 이름의 카테고리가 있습니다.".into());
    }
    let position = match id {
        Some(id) => {
            all.iter()
                .find(|v| v.id == id)
                .ok_or("카테고리를 찾지 못했습니다.")?
                .position
        }
        None => all.len() as u32,
    };
    let value = Category {
        id: record_id(id.map(str::to_owned))?,
        name,
        color: color.to_ascii_uppercase(),
        position,
    };
    c.execute("INSERT INTO calendar_categories VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET name=excluded.name,color=excluded.color", params![value.id,value.name,value.color,value.position]).map_err(database_error)?;
    Ok(value)
}
