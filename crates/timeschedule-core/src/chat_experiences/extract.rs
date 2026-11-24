#[derive(Clone, Copy)]
pub(crate) struct Block<'a> {
    pub(crate) role: &'a str,
    pub(crate) line: usize,
    pub(crate) text: &'a str,
}

pub(crate) fn blocks(body: &str) -> Vec<Block<'_>> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut current = None;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let header = line
            .trim_end()
            .strip_prefix("## ")
            .and_then(|h| h.split_once(" · 원문 "))
            .filter(|(r, _)| matches!(*r, "사용자" | "AI" | "검사 출력"))
            .and_then(|(r, n)| Some((r, n.strip_suffix('행')?.parse::<usize>().ok()?)));
        if let Some((role, n)) = header {
            if let Some((role, line)) = current {
                result.push(Block {
                    role,
                    line,
                    text: body[start..offset].trim(),
                });
            }
            current = Some((role, n));
            start = offset + line.len();
        }
        offset += line.len();
    }
    if let Some((role, line)) = current {
        result.push(Block {
            role,
            line,
            text: body[start..].trim(),
        });
    }
    result
}
