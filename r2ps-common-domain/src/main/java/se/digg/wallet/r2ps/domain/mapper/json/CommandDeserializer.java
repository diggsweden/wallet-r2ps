package se.digg.wallet.r2ps.domain.mapper.json;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.databind.DeserializationContext;
import com.fasterxml.jackson.databind.JsonDeserializer;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import se.digg.wallet.r2ps.domain.command.Command;
import se.digg.wallet.r2ps.domain.command.RegisterServerWallet;

import java.io.IOException;

public class CommandDeserializer extends JsonDeserializer<Command> {

  @Override
  public Command deserialize(JsonParser p, DeserializationContext ctxt) throws IOException {
    JsonNode node = p.getCodec().readTree(p);

    // Extract eventType from nested metadata
    JsonNode metadataNode = node.get("metadata");
    if (metadataNode == null || !metadataNode.has("commandType")) {
      throw new IOException("Missing metadata.eventType");
    }

    String eventType = metadataNode.get("commandType").asText();

    // Determine the concrete class based on eventType
    Class<? extends Command>  commandClass = switch (eventType) {
      case "RegisterServerWallet" -> RegisterServerWallet.class;
      // Add other event types here
      default -> throw new IllegalArgumentException("Unknown event type: " + eventType);
    };

    // Deserialize to the concrete type
    ObjectMapper mapper = (ObjectMapper) p.getCodec();
    return mapper.treeToValue(node, commandClass);
  }
}
