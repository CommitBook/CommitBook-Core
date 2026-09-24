use super::*;

#[test]
fn enum_values_round_trip_through_their_config_text() {
    for mode in ConflictMode::ALL {
        assert_eq!(mode.as_str().parse::<ConflictMode>().unwrap(), *mode);
    }
    for agent in CommitAgent::ALL {
        assert_eq!(agent.to_string().parse::<CommitAgent>().unwrap(), *agent);
    }
    assert_eq!("github_app".parse::<Auth>().unwrap(), Auth::GithubApp);
}

#[test]
fn unknown_enum_value_lists_the_allowed_values() {
    let error = "auto".parse::<ConflictMode>().unwrap_err().to_string();
    assert!(error.contains("Unknown conflict mode `auto`"), "{error}");
    assert!(error.contains("both, manual, ai, review"), "{error}");
}

#[test]
fn commit_agent_any_names_no_single_agent() {
    assert_eq!(CommitAgent::Any.agent(), None);
    assert_eq!(CommitAgent::Gemini.agent(), Some(Agent::Gemini));
}

#[test]
fn log_keep_parses_days_and_forever() {
    assert_eq!("7d".parse::<LogKeep>().unwrap(), LogKeep::Days(7));
    assert_eq!("forever".parse::<LogKeep>().unwrap(), LogKeep::Forever);
    assert_eq!(LogKeep::Days(30).to_string(), "30d");
    assert_eq!(LogKeep::Forever.days(), None);
    assert_eq!(LogKeep::default(), LogKeep::Days(30));
}

#[test]
fn log_keep_rejects_zero_bare_numbers_and_huge_values() {
    for input in ["0d", "7", "d", "-1d", "3651d", "30 days"] {
        assert!(
            input.parse::<LogKeep>().is_err(),
            "{input} should be rejected"
        );
    }
}
